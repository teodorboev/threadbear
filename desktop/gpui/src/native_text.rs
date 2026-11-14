// The Mac's selection consumers for GPUI's own text. AppKit gets a plain
// snapshot; a Service's answer comes back through the foreground executor,
// then the original textarea/range/text and current focus are checked again.
use gpui_kit::*;
#[cfg(target_os="macos")] use gpui_kit::component::WindowExt as _;
gpui_kit::actions!(native_text, [LookUp, Services, WritingTools, Translate]);

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use crate::text_services::{self, SelectionSource};
    use futures::{channel::mpsc, StreamExt};
    use gpui_kit::component::WindowExt as _;
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    use std::{cell::RefCell, collections::HashMap, ffi::c_void, sync::{atomic::{AtomicU64, Ordering}, Mutex}};
    const LIMIT: usize = 1024 * 1024;
    struct WatchedInput { id: EntityId, _subscription: Subscription }
    #[cfg(feature="automation")]
    static LOOKUP_REQUESTS: AtomicU64 = AtomicU64::new(0);
    #[cfg(feature="automation")]
    static LOOKUP_SHORTCUTS: AtomicU64 = AtomicU64::new(0);
    static NEXT: AtomicU64 = AtomicU64::new(1);
    static ANSWERS: Mutex<Option<mpsc::Sender<(u64, String)>>> = Mutex::new(None);
    #[derive(Clone)]
    struct Snapshot { owner: u64, token: u64, words: String, source: SelectionSource, window: AnyWindowHandle }
    thread_local! {
        #[cfg(feature="automation")]
        static LOOKUP_WORDS: RefCell<Option<String>> = RefCell::new(None);
        static WATCHED: RefCell<HashMap<u64, WatchedInput>> = RefCell::new(HashMap::new());
        static CURRENT: RefCell<HashMap<u64, Snapshot>> = RefCell::new(HashMap::new());
        static REQUESTS: RefCell<HashMap<u64, Snapshot>> = RefCell::new(HashMap::new());
        static PRESSURE: RefCell<HashMap<u64, bool>> = RefCell::new(HashMap::new());
    }
    extern "C" {
        fn tb_selection_update(view: *mut c_void, window: u64, token: u64, words: *const u8, length: usize, editable: bool);
        fn tb_selection_translate_available() -> bool;
        fn tb_selection_translate(view:*mut c_void, request:u64, words:*const u8, length:usize, x:f64, y:f64, editable:bool) -> bool;
        fn tb_selection_writing_available() -> bool;
        fn tb_selection_writing(view: *mut c_void) -> bool;
        fn tb_selection_share(view:*mut c_void, words:*const u8, length:usize, x:f64, y:f64);
        fn tb_selection_services(view: *mut c_void, x: f64, y: f64) -> bool;
        fn tb_selection_retain(view: *mut c_void) -> *mut c_void;
        fn tb_selection_lookup(view: *mut c_void, words: *const u8, length: usize, x: f64, y: f64);
    }
    fn view(window: &Window) -> Option<*mut c_void> {
        match raw_window_handle::HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::AppKit(handle) => Some(handle.ns_view.as_ptr()),
            _ => None,
        }
    }
    fn same(a:&SelectionSource,b:&SelectionSource)->bool {
        match(a,b) {
            (SelectionSource::Reply,SelectionSource::Reply)=>true,
            (SelectionSource::TextBox{input:a,range:ar},SelectionSource::TextBox{input:b,range:br})=>a.entity_id()==b.entity_id()&&ar==br,
            (SelectionSource::TextField{input:a,range:ar},SelectionSource::TextField{input:b,range:br})=>a.entity_id()==b.entity_id()&&ar==br,
            (SelectionSource::Editor{input:a,range:ar},SelectionSource::Editor{input:b,range:br})=>a.entity_id()==b.entity_id()&&ar==br,
            _=>false,
        }
    }
    struct SelectionServices;
    impl gpui_kit::base::RootPlugin for SelectionServices {
        fn decorate(&self, surface:AnyElement, _: &gpui_kit::base::Root, _: &mut Window, _: &mut App) -> impl IntoElement {
            // Keyboard and action listeners belong above the focused input.
            // The plugin's painted overlay is its sibling, outside that path.
            div().size_full()
                .on_action(|_:&LookUp,w,cx|lookup(w,cx))
                .on_action(|_:&Translate,w,cx|translate(w,cx))
                .on_action(|_:&WritingTools,w,cx|writing_tools(w,cx))
                .on_action(|_:&Services,w,cx|services(w,cx))
                .capture_key_down(|event,w,cx|{
                    let key=&event.keystroke;
                    if key.key=="d" && key.modifiers.platform && key.modifiers.control && !key.modifiers.alt && !key.modifiers.shift
                        && text_services::app_has_keyboard(w) && !crate::terminal::has_focus(w,cx) && has_lookup(w,cx) {
                        #[cfg(feature="automation")]
                        LOOKUP_SHORTCUTS.fetch_add(1,Ordering::Relaxed);
                        let handle=Window::window_handle(w);
                        cx.defer(move|cx|{let _=handle.update(cx,|_,w,cx|lookup(w,cx));});
                        cx.stop_propagation();
                    }else{queue_refresh(w,cx)}
                })
                .on_key_up(|_,w,cx|queue_refresh(w,cx))
                .child(surface)
        }
    }
    impl Render for SelectionServices {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement { watch() }
    }
    pub(super) fn init(cx: &mut App) {
        // Every kit window participates, including Settings and reply windows.
        // Root owns this overlay once, independently of cached app panels.
        gpui_kit::base::Root::register_plugin(cx, |_, _| SelectionServices);
        gpui_kit::component::input::set_default_context_menu_extension(field_context_items, cx);
        let (tx, mut rx) = mpsc::channel(32);
        *ANSWERS.lock().unwrap() = Some(tx);
        cx.spawn(async move |cx| {
            while let Some((request, answer)) = rx.next().await {
                let original = REQUESTS.with_borrow_mut(|requests| requests.remove(&request));
                let Some(original) = original else { continue };
                let _ = cx.update(move |cx| original.window.update(cx, |_, window, cx| {
                    let still = text_services::front_selection(window,cx).is_some_and(|(words,source)| words==original.words && same(&source,&original.source));
                    if !still || !text_services::replace_if_unchanged(&original.source,&original.words,&answer,window,cx) {
                        window.push_notification("Your selection changed, so the service's answer wasn't inserted.",cx);
                    }
                }));
            }
        }).detach();
    }
    // These callbacks never enter GPUI. They only freeze the latest foreground
    // snapshot or queue bytes for the foreground task above.
    #[no_mangle]
    extern "C" fn tb_services_freeze(window: u64, token: u64) -> u64 {
        let snapshot = CURRENT.with_borrow(|all| all.get(&window).filter(|s|s.token==token).cloned());
        let Some(snapshot)=snapshot else { return 0 };
        REQUESTS.with_borrow_mut(|all| {
            if all.len()>=1000 { return 0 }
            let id=NEXT.fetch_add(1,Ordering::Relaxed); all.insert(id,snapshot); id
        })
    }
    #[no_mangle]
    extern "C" fn tb_services_discard(request:u64) { REQUESTS.with_borrow_mut(|all|{all.remove(&request);}); }
    #[no_mangle]
    extern "C" fn tb_services_closed(window:u64) {
        WATCHED.with_borrow_mut(|all|{all.remove(&window);});
        WINDOWS.with_borrow_mut(|all|{all.remove(&window);});
        CURRENT.with_borrow_mut(|all|{all.remove(&window);});
        REQUESTS.with_borrow_mut(|all|all.retain(|_,s|s.owner!=window));
        PRESSURE.with_borrow_mut(|all|{all.remove(&window);});
    }
    #[no_mangle]
    unsafe extern "C" fn tb_services_return(request:u64, words:*const u8, length:usize)->bool {
        if length>LIMIT || (words.is_null() && length!=0) || !REQUESTS.with_borrow(|all|all.contains_key(&request)) {return false}
        let bytes=if length==0 {&[][..]} else {unsafe{std::slice::from_raw_parts(words,length)}};
        let Ok(words)=std::str::from_utf8(bytes) else{return false};
        ANSWERS.lock().unwrap().as_mut().is_some_and(|tx|tx.try_send((request,words.to_owned())).is_ok())
    }
    fn watch_input(window:&mut Window,cx:&mut App) {
        use gpui_kit::component::input::AnyInputState;
        let key=crate::thread::window_key(window);
        let input=window.focused_input(cx).or_else(||text_services::focused_text_box(window,cx).map(AnyInputState::Textarea));
        macro_rules! observe {
            ($input:expr)=>{{
                let input=$input;
                let id=input.entity_id();
                if WATCHED.with_borrow(|all|all.get(&key).is_some_and(|w|w.id==id)){return}
                let state=input.read(cx);
                let stamp=std::rc::Rc::new(RefCell::new((state.selected_range(),state.document_revision(),state.is_sensitive(),state.has_composition(),state.is_single_selection(),state.presentation().is_editable())));
                let handle=Window::window_handle(window);
                let subscription=cx.observe(&input,move|input,cx|{
                    let state=input.read(cx);
                    let next=(state.selected_range(),state.document_revision(),state.is_sensitive(),state.has_composition(),state.is_single_selection(),state.presentation().is_editable());
                    if *stamp.borrow()!=next {
                        *stamp.borrow_mut()=next;
                        cx.defer(move|cx|{let _=handle.update(cx,|_,window,cx|refresh(window,cx));});
                    }
                });
                WATCHED.with_borrow_mut(|all|{all.insert(key,WatchedInput{id,_subscription:subscription});});
            }};
        }
        match input {
            Some(AnyInputState::Input(input))=>observe!(input),
            Some(AnyInputState::Textarea(input))=>observe!(input),
            Some(AnyInputState::Editor(input))=>observe!(input),
            _=>{WATCHED.with_borrow_mut(|all|{all.remove(&key);});},
        }
    }
    fn refresh(window:&mut Window,cx:&mut App) {
        watch_input(window,cx);
        let Some(view)=view(window) else{return};
        let key=crate::thread::window_key(window);
        let next=text_services::front_selection(window,cx).filter(|(words,_)|words.len()<=LIMIT);
        let changed=CURRENT.with_borrow(|all| match (all.get(&key),next.as_ref()) {
            (Some(old),Some((words,source)))=>old.words!=*words || !same(&old.source,source),
            (None,None)=>false,
            _=>true,
        });
        if !changed {
            // Also install for an empty selection, so closing the native view
            // releases the per-window input observer. This call copies no text.
            CURRENT.with_borrow(|all|unsafe{match all.get(&key) {
                Some(s)=>tb_selection_update(view,key,s.token,s.words.as_ptr(),s.words.len(),s.source.is_editable(cx)&&!s.words.contains('\u{fffc}')),
                None=>tb_selection_update(view,key,0,std::ptr::null(),0,false),
            }});
            return
        }
        if let Some((words,source))=next {
            let token=NEXT.fetch_add(1,Ordering::Relaxed);
            let editable=source.is_editable(cx) && !words.contains('\u{fffc}');
            let snapshot=Snapshot{owner:key,token,words,source,window:Window::window_handle(window)};
            unsafe{tb_selection_update(view,key,token,snapshot.words.as_ptr(),snapshot.words.len(),editable)};
            CURRENT.with_borrow_mut(|all|{all.insert(key,snapshot);});
        } else {
            CURRENT.with_borrow_mut(|all|{all.remove(&key);});
            unsafe{tb_selection_update(view,key,0,std::ptr::null(),0,false)};
        }
    }
    // A frame can be a caret blink or an unrelated sidebar update. Watch
    // focus and actual selection/document changes; never recopy a long
    // selected draft merely because the root was painted again.
    struct WindowWatch {
        focus: (Option<EntityId>, bool, bool),
        _selection: Subscription,
    }
    thread_local! { static WINDOWS: RefCell<HashMap<u64,WindowWatch>> = RefCell::new(HashMap::new()); }
    fn watch_window(window:&mut Window,cx:&mut App) {
        use gpui_kit::component::input::AnyInputState;
        let key=crate::thread::window_key(window);
        let input=window.focused_input(cx).or_else(||text_services::focused_text_box(window,cx).map(AnyInputState::Textarea));
        let id=input.map(|input|match input {
            AnyInputState::Input(input)=>input.entity_id(),
            AnyInputState::Textarea(input)=>input.entity_id(),
            AnyInputState::Editor(input)=>input.entity_id(),
            AnyInputState::Otp(input)=>input.entity_id(),
        });
        let eligible=text_services::app_has_keyboard(window) && !crate::terminal::has_focus(window,cx);
        // A markdown replacement clears mouse selections during prepaint
        // without notifying the touch observer. Notice that cheap state bit
        // here so AppKit cannot keep words whose highlight disappeared.
        let reply_selected=id.is_none() && eligible && gpui_kit::base::TextSelection::has_selection(window,cx);
        let focus=(id,eligible,reply_selected);
        let changed=WINDOWS.with_borrow_mut(|all|{
            if let Some(watch)=all.get_mut(&key){
                if watch.focus==focus{return false}
                watch.focus=focus;return true
            }
            let handle=Window::window_handle(window);
            let selection=gpui_kit::base::TextSelection::observe_touch_selection(window,cx,move|cx|{
                cx.defer(move|cx|{let _=handle.update(cx,|_,window,cx|refresh(window,cx));});
            });
            all.insert(key,WindowWatch{focus,_selection:selection});true
        });
        watch_input(window,cx);
        if changed{queue_refresh(window,cx)}
    }

    fn queue_refresh(window:&Window,cx:&mut App) {
        let handle=Window::window_handle(window);
        cx.defer(move |cx|{let _=handle.update(cx,|_,window,cx|refresh(window,cx));});
    }
    fn show_definition(words:String,at:Point<Pixels>,window:&Window,cx:&mut App) {
        if words.is_empty() || words.len()>4096{return}
        let Some(view)=view(window) else{return};
        let retained=unsafe{tb_selection_retain(view)} as usize;
        cx.spawn(async move |_| {
            #[cfg(feature="automation")] {
                LOOKUP_REQUESTS.fetch_add(1,Ordering::Relaxed);
                LOOKUP_WORDS.with_borrow_mut(|last|*last=Some(words.clone()));
            }
            unsafe{tb_selection_lookup(retained as *mut c_void,words.as_ptr(),words.len(),f32::from(at.x) as f64,f32::from(at.y) as f64)}
        }).detach();
    }
    fn word_at(at:Point<Pixels>,window:&mut Window,cx:&mut App)->Option<String> {
        if !text_services::app_has_keyboard(window) || crate::terminal::has_focus(window,cx){return None}
        use gpui_kit::component::input::AnyInputState;
        macro_rules! word {
            ($input:expr)=>{{
                let input=$input;let state=input.read(cx);
                if state.is_sensitive(){return None}
                if state.input_bounds().contains(&at) {
                    let text=state.value();
                    let range=threadbear_core::spelling::word_at(&text,state.offset_for_point(at));
                    if state.range_to_bounds(&range).is_some_and(|bounds|bounds.contains(&at)){return text.get(range).filter(|word|!word.is_empty()).map(str::to_owned)}
                }
            }};
        }
        match window.focused_input(cx) {
            Some(AnyInputState::Input(input))=>word!(input),
            Some(AnyInputState::Textarea(input))=>word!(input),
            Some(AnyInputState::Editor(input))=>word!(input),
            Some(AnyInputState::Otp(_))=>return None,
            None=>{},
        }
        gpui_kit::base::TextSelection::word_at(at,window,cx)
    }
    pub(super) fn lookup(window:&mut Window,cx:&mut App) {
        if let Some((words,source))=text_services::front_selection(window,cx).filter(|(words,_)|words.len()<=4096) {
            let at=match source {
                SelectionSource::TextBox{input,range}=>input.upgrade().and_then(|input|input.read(cx).range_to_bounds(&range)).map(|b|point(b.left(),b.bottom())),
                SelectionSource::TextField{input,range}=>input.upgrade().and_then(|input|input.read(cx).range_to_bounds(&range)).map(|b|point(b.left(),b.bottom())),
                SelectionSource::Editor{input,range}=>input.upgrade().and_then(|input|input.read(cx).range_to_bounds(&range)).map(|b|point(b.left(),b.bottom())),
                SelectionSource::Reply=>gpui_kit::base::TextSelection::selection_cursor(window,cx),
            }.unwrap_or(window.mouse_position());
            show_definition(words,at,window,cx);
        }else{
            let at=window.mouse_position();
            if let Some(words)=word_at(at,window,cx){show_definition(words,at,window,cx)}
        }
    }
    pub(super) fn has_selection(window:&mut Window,cx:&mut App)->bool {
        text_services::front_selection(window,cx).is_some_and(|(words,_)|!words.is_empty()&&words.len()<=LIMIT)
    }
    pub(super) fn has_lookup(window:&mut Window,cx:&mut App)->bool {
        text_services::front_selection(window,cx).is_some_and(|(words,_)|!words.is_empty()&&words.len()<=4096) || word_at(window.mouse_position(),window,cx).is_some()
    }
    pub(super) fn services(window:&mut Window,cx:&mut App) {
        refresh(window,cx);
        let Some(view)=view(window) else{return};
        let retained=unsafe{tb_selection_retain(view)} as usize;
        let at=window.mouse_position();
        let handle=Window::window_handle(window);
        cx.spawn(async move|cx|{
            let shown=unsafe{tb_selection_services(retained as *mut c_void,f32::from(at.x) as f64,f32::from(at.y) as f64)};
            let _=cx.update(|cx|handle.update(cx,|_,window,cx|{
                if !shown{window.push_notification("No text services are enabled in macOS.",cx);}
                window.refresh();
            }));
        }).detach();
    }
    pub(super) fn share(window:&mut Window,cx:&mut App) {
        let Some((words,_))=text_services::front_selection(window,cx) else{return};
        let Some(view)=view(window) else{return};
        let retained=unsafe{tb_selection_retain(view)} as usize;
        let at=window.mouse_position();
        cx.spawn(async move|_|unsafe{tb_selection_share(retained as *mut c_void,words.as_ptr(),words.len(),f32::from(at.x) as f64,f32::from(at.y) as f64)}).detach();
    }
    pub(super) fn translate_available()->bool {unsafe{tb_selection_translate_available()}}
    pub(super) fn translate(window:&mut Window,cx:&mut App) {
        refresh(window,cx);
        let key=crate::thread::window_key(window);
        let snapshot=CURRENT.with_borrow(|all|all.get(&key).cloned());
        let Some(snapshot)=snapshot else{return};
        let Some(view)=view(window) else{return};
        let request=tb_services_freeze(key,snapshot.token);
        if request==0{return}
        let retained=unsafe{tb_selection_retain(view)} as usize;
        let at=window.mouse_position();
        let editable=snapshot.source.is_editable(cx)&&!snapshot.words.contains('\u{fffc}');
        let handle=Window::window_handle(window);
        cx.spawn(async move|cx|{
            let shown=unsafe{tb_selection_translate(retained as *mut c_void,request,snapshot.words.as_ptr(),snapshot.words.len(),f32::from(at.x) as f64,f32::from(at.y) as f64,editable)};
            if !shown {let _=cx.update(|cx|handle.update(cx,|_,window,cx|window.push_notification("Translate needs macOS 14.4 or later.",cx)));}
        }).detach();
    }
    pub(super) fn writing_tools(window:&mut Window,cx:&mut App) {
        refresh(window,cx);
        let Some(view)=view(window) else{return};
        let retained=unsafe{tb_selection_retain(view)} as usize;
        let handle=Window::window_handle(window);
        cx.spawn(async move|cx|{
            let shown=unsafe{tb_selection_writing(retained as *mut c_void)};
            if !shown {let _=cx.update(|cx|handle.update(cx,|_,window,cx|window.push_notification("Writing Tools couldn't open for this selection.",cx)));}
        }).detach();
    }
    pub(super) fn writing_tools_available()->bool {unsafe{tb_selection_writing_available()}}
    pub(super) fn watch()->AnyElement {
        canvas(|_,_,_|{},|_,_,window,cx|{
            watch_window(window,cx);
            window.on_mouse_event(|_:&MouseUpEvent,phase,window,cx|{if phase.capture(){PRESSURE.with_borrow_mut(|all|{all.insert(crate::thread::window_key(window),false);});queue_refresh(window,cx)}});
            window.on_mouse_event(|_:&MouseDownEvent,phase,window,cx|{if phase.capture(){PRESSURE.with_borrow_mut(|all|{all.insert(crate::thread::window_key(window),false);});queue_refresh(window,cx)}});
            window.on_mouse_event(|event:&MousePressureEvent,phase,window,cx|{
                if !phase.capture(){return}
                let high=event.stage==PressureStage::Force;
                let key=crate::thread::window_key(window);
                let began=PRESSURE.with_borrow_mut(|all|{let previous=all.insert(key,high).unwrap_or(false);high&&!previous});
                if began {
                    let at=event.position;
                    let handle=Window::window_handle(window);
                    cx.defer(move|cx|{let _=handle.update(cx,|_,w,cx|{if let Some(words)=word_at(at,w,cx){show_definition(words,at,w,cx)}});});
                    cx.stop_propagation();
                }
            });
        }).absolute().size_0().into_any_element()
    }
    #[cfg(feature="automation")]
    pub(super) async fn automation(window:AnyWindowHandle,cmd:&serde_json::Value,cx:&mut AsyncApp)->Result<serde_json::Value,String> {
        use serde_json::json;
        extern "C" {
            fn tb_selection_test_begin(view:*mut c_void,returning:bool,out:*mut u8,capacity:usize,length:*mut usize)->u64;
            fn tb_selection_test_return(request:u64,words:*const u8,length:usize)->bool;
            fn tb_selection_test_discard(request:u64);
            fn tb_selection_test_windows(out:*mut u8,capacity:usize)->usize;
            fn tb_selection_test_dismiss();
        }
        match cmd["op"].as_str().unwrap_or("state") {
            "state"=>cx.update_window(window,|_,window,cx|{
                let selected=text_services::front_selection(window,cx);
                let cached=CURRENT.with_borrow(|all|all.get(&crate::thread::window_key(window)).map(|s|s.words.clone()));
                json!({"lookupRequests":LOOKUP_REQUESTS.load(Ordering::Relaxed),"lookupShortcuts":LOOKUP_SHORTCUTS.load(Ordering::Relaxed),"lookupWords":LOOKUP_WORDS.with_borrow(|last|last.clone()),"words":selected.as_ref().map(|(words,_)|words),"snapshotWords":cached,"editable":selected.as_ref().is_some_and(|(_,source)|source.is_editable(cx)),"translate":translate_available(),"writingTools":writing_tools_available()})
            }).map_err(|e|e.to_string()),
            "begin"=>{
                let view=cx.update_window(window,|_,window,_|view(window).map(|view|unsafe{tb_selection_retain(view)} as usize)).map_err(|e|e.to_string())?.ok_or("no AppKit view")?;
                let mut bytes=vec![0;LIMIT];let mut length=0;
                // Out of the GPUI update: AppKit may validate the responder.
                let request=unsafe{tb_selection_test_begin(view as *mut c_void,cmd["returning"].as_bool().unwrap_or(true),bytes.as_mut_ptr(),bytes.len(),&mut length)};
                if request==0{return Err("AppKit has no requestor for this selection".into())}
                bytes.truncate(length);
                Ok(json!({"request":request,"words":String::from_utf8(bytes).map_err(|e|e.to_string())?}))
            },
            "return"=>{
                let request=cmd["request"].as_u64().ok_or("request missing")?;
                let words=cmd["text"].as_str().ok_or("text missing")?;
                let accepted=unsafe{tb_selection_test_return(request,words.as_ptr(),words.len())};
                cx.background_executor().timer(std::time::Duration::from_millis(30)).await;
                Ok(json!({"accepted":accepted}))
            },
            "discard"=>{unsafe{tb_selection_test_discard(cmd["request"].as_u64().ok_or("request missing")?)};Ok(json!("ok"))},
            "dismiss"=>{unsafe{tb_selection_test_dismiss()};Ok(json!("ok"))},
            "windows"=>{
                let mut bytes=vec![0;65536];let length=unsafe{tb_selection_test_windows(bytes.as_mut_ptr(),bytes.len())};bytes.truncate(length);
                serde_json::from_slice(&bytes).map_err(|e|e.to_string())
            },
            op @ ("lookup"|"translate"|"writing"|"services")=>{
                cx.update_window(window,|_,window,cx|match op{"lookup"=>lookup(window,cx),"translate"=>translate(window,cx),"writing"=>writing_tools(window,cx),_=>services(window,cx)}).map_err(|e|e.to_string())?;
                Ok(json!("ok"))
            },
            _=>Err("unknown native-text operation".into()),
        }
    }

}

pub(crate) fn init(cx:&mut App) { #[cfg(target_os="macos")] mac::init(cx); #[cfg(not(target_os="macos"))] let _=cx; }
pub(crate) fn watch()->AnyElement {
    #[cfg(target_os="macos")] {return mac::watch()}
    #[cfg(not(target_os="macos"))] {gpui_kit::div().size_0().into_any_element()}
}
pub(crate) fn lookup(window:&mut Window,cx:&mut App) { #[cfg(target_os="macos")] mac::lookup(window,cx); #[cfg(not(target_os="macos"))] let _=(window,cx); }

pub(crate) fn has_selection(window:&mut Window,cx:&mut App)->bool {
    #[cfg(target_os="macos")] {return mac::has_selection(window,cx)}
    #[cfg(not(target_os="macos"))] {let _=(window,cx);false}
}
pub(crate) fn has_lookup(window:&mut Window,cx:&mut App)->bool {
    #[cfg(target_os="macos")] {return mac::has_lookup(window,cx)}
    #[cfg(not(target_os="macos"))] {let _=(window,cx);false}
}
pub(crate) fn services(window:&mut Window,cx:&mut App) { #[cfg(target_os="macos")] mac::services(window,cx); #[cfg(not(target_os="macos"))] let _=(window,cx); }
// PopupMenu invokes handlers before restoring the original input's focus.
// Native requestors must be read after dismissal, with the old focus back.
#[cfg(target_os="macos")]
fn after_menu(window:&Window,cx:&mut App,expected:std::sync::Arc<str>,perform:fn(&mut Window,&mut App)) {
    let handle=Window::window_handle(window);
    cx.defer(move|cx|{let _=handle.update(cx,|_,window,cx|{
        if crate::text_services::reply_menu_selection(window,cx).as_deref()==Some(&*expected) {perform(window,cx)}
        else {window.push_notification("Your selection changed, so the text action wasn't opened.",cx);}
    });});
}
pub(crate) fn context_items(menu:gpui_kit::component::menu::PopupMenu,pane:WeakEntity<crate::thread::ThreadPane>,window:&mut Window,cx:&mut App)->gpui_kit::component::menu::PopupMenu {
    #[cfg(target_os="macos")] {
        use gpui_kit::component::menu::PopupMenuItem;
        let Some(words)=crate::text_services::reply_menu_selection(window,cx) else{return menu};
        let html = pane.upgrade().and_then(|pane| crate::selection_bar::selection_now(&pane,window,cx))
            .filter(|(selected,_)| selected == &words).map(|(_,html)|html)
            .unwrap_or_else(||threadbear_core::selection_copy::selection_html(&words));
        let words:std::sync::Arc<str>=words.into();
        let copied=words.clone();let searched=words.clone();let looked=words.clone();
        let shared=words.clone();let serviced=words.clone();let spoken=words.clone();
        let translated=words.clone();let written=words;
        let menu=menu.item(PopupMenuItem::new("Copy").on_click(move|_,_,cx|crate::selection_bar::put_on_clipboard(&copied,&html,cx)))
            .item(PopupMenuItem::new("Look Up").on_click(move|_,window,cx|after_menu(window,cx,looked.clone(),lookup)))
            .item(PopupMenuItem::new("Search with Google").on_click(move|_,_,cx|{
                crate::open_gate::open_web_page(&format!("https://www.google.com/search?q={}",crate::web_runtime::percent_encode(&searched)),cx);
            }))
            .item(PopupMenuItem::new("Share…").on_click(move|_,window,cx|after_menu(window,cx,shared.clone(),mac::share)))
            .item(PopupMenuItem::new("Services…").on_click(move|_,window,cx|after_menu(window,cx,serviced.clone(),services)))
            .item(PopupMenuItem::new("Start Speaking").on_click(move|_,window,cx|after_menu(window,cx,spoken.clone(),|w,cx|{
                if let Some((words,_))=crate::text_services::front_selection(w,cx){crate::text_services::speak(&words);}
            })))
            .item(PopupMenuItem::new("Stop Speaking").disabled(!crate::text_services::speaking()).on_click(|_,_,_|crate::text_services::stop_speaking()));
        let menu=if translate_available(){menu.item(PopupMenuItem::new("Translate").on_click(move|_,window,cx|after_menu(window,cx,translated.clone(),mac::translate)))}else{menu};
        return if writing_tools_available(){menu.item(PopupMenuItem::new("Writing Tools").on_click(move|_,window,cx|after_menu(window,cx,written.clone(),mac::writing_tools))).separator()}else{menu.separator()}
    }
    #[cfg(not(target_os="macos"))] {let _=(pane,window,cx);menu}
}

pub(crate) fn writing_tools_available()->bool {
    #[cfg(target_os="macos")] {return mac::writing_tools_available()}
    #[cfg(not(target_os="macos"))] {false}
}

pub(crate) fn translate_available()->bool {
    #[cfg(target_os="macos")] {return mac::translate_available()}
    #[cfg(not(target_os="macos"))] {false}
}

#[cfg(target_os="macos")]
fn field_context_items(menu:gpui_kit::component::native_menu::NativeMenu,window:&mut Window,cx:&mut App)->gpui_kit::component::native_menu::NativeMenu {
    let selected=has_selection(window,cx);
    let lookup=has_lookup(window,cx);
    // In particular, a password field must not borrow a reply's old selection.
    if !selected && !lookup { return menu }
    let menu=menu.separator().menu_with_disabled("Look Up",!lookup,Box::new(LookUp))
        .menu_with_disabled("Services…",!selected,Box::new(Services));
    let menu=if translate_available(){menu.menu_with_disabled("Translate",!selected,Box::new(Translate))}else{menu};
    if writing_tools_available(){menu.menu_with_disabled("Writing Tools",!selected,Box::new(WritingTools))}else{menu}
}

#[cfg(feature="automation")]
pub(crate) async fn automation(window:AnyWindowHandle,cmd:&serde_json::Value,cx:&mut AsyncApp)->Result<serde_json::Value,String> {
    #[cfg(target_os="macos")] {return mac::automation(window,cmd,cx).await}
    #[cfg(not(target_os="macos"))] {let _=(window,cmd,cx);Err("native text services need macOS".into())}
}
