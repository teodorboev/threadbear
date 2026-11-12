// The AppKit glue for running CEF inside the GPUI app on a Mac. Adapted from
// Ghostex's apps/desktop/native/macos/GpuiCefAppKitHooks.m by way of the
// gpui-web-spike branch (desktop/web-spike/native/appkit_hooks.m there), MIT
// licensed:
//
//   Copyright 2026 Mohamad Yahia (maddada)
//
//   Permission is hereby granted, free of charge, to any person obtaining a
//   copy of this software and associated documentation files (the "Software"),
//   to deal in the Software without restriction, including without limitation
//   the rights to use, copy, modify, merge, publish, distribute, sublicense,
//   and/or sell copies of the Software, and to permit persons to whom the
//   Software is furnished to do so, subject to the following conditions:
//
//   The above copyright notice and this permission notice shall be included in
//   all copies or substantial portions of the Software.
//
//   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//   IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//   FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//   AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//   LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
//   FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//   DEALINGS IN THE SOFTWARE.
#import <AppKit/AppKit.h>
#import <objc/runtime.h>
#include <stdbool.h>

// CEF's message pump traps unless NSApp conforms to CefAppProtocol
// (cef_application_mac.h). GPUI owns the NSApplication subclass, so the
// protocol and the isHandlingSendEvent state are added at run time instead.
@protocol CrAppProtocol
- (BOOL)isHandlingSendEvent;
@end
@protocol CrAppControlProtocol <CrAppProtocol>
- (void)setHandlingSendEvent:(BOOL)handlingSendEvent;
@end
@protocol CefAppProtocol <CrAppControlProtocol>
@end

static BOOL g_handlingSendEvent = NO;

@interface NSApplication (ThreadbearCEF) <CefAppProtocol>
- (BOOL)isHandlingSendEvent;
- (void)setHandlingSendEvent:(BOOL)handlingSendEvent;
- (void)threadbearCefSendEvent:(NSEvent *)event;
@end

@implementation NSApplication (ThreadbearCEF)
- (BOOL)isHandlingSendEvent {
  return g_handlingSendEvent;
}
- (void)setHandlingSendEvent:(BOOL)handlingSendEvent {
  g_handlingSendEvent = handlingSendEvent;
}
- (void)threadbearCefSendEvent:(NSEvent *)event {
  BOOL was = g_handlingSendEvent;
  g_handlingSendEvent = YES;
  // Swizzled: this calls the original -sendEvent:.
  [self threadbearCefSendEvent:event];
  g_handlingSendEvent = was;
}
@end

void tb_cef_install_app_hooks(void) {
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    Class baseClass = [NSApplication class];
    Method original = class_getInstanceMethod(baseClass, @selector(sendEvent:));
    Method swapped = class_getInstanceMethod(baseClass, @selector(threadbearCefSendEvent:));
    if (original && swapped) {
      method_exchangeImplementations(original, swapped);
    }
    class_addProtocol(baseClass, @protocol(CrAppProtocol));
    class_addProtocol(baseClass, @protocol(CrAppControlProtocol));
    class_addProtocol(baseClass, @protocol(CefAppProtocol));
  });
  // NSApp is GPUI's subclass; Chromium checks the concrete class.
  if (NSApp) {
    Class appClass = object_getClass(NSApp);
    class_addProtocol(appClass, @protocol(CrAppProtocol));
    class_addProtocol(appClass, @protocol(CrAppControlProtocol));
    class_addProtocol(appClass, @protocol(CefAppProtocol));
  }
}

// Implemented in Rust (host.rs): starts CEF, then runs Chromium's message
// loop for the rest of the process.
extern void tb_cef_run_message_loop(void);

static BOOL g_startPending = NO;
static BOOL g_loopRunning = NO;

static void tb_cef_schedule_start(CFTimeInterval delay) {
  CFRunLoopTimerRef timer = CFRunLoopTimerCreateWithHandler(
      kCFAllocatorDefault, CFAbsoluteTimeGetCurrent() + delay, 0, 0, 0,
      ^(CFRunLoopTimerRef firedTimer) {
        (void)firedTimer;
        if (!g_startPending) {
          return;
        }
        // Not inside a modal session (an alert, a save panel): Chromium's
        // loop would end up nested in it.
        if (NSApp.modalWindow != nil) {
          tb_cef_schedule_start(0.25);
          return;
        }
        g_startPending = NO;
        g_loopRunning = YES;
        tb_cef_run_message_loop();
        g_loopRunning = NO;
      });
  CFRunLoopAddTimer(CFRunLoopGetMain(), timer, kCFRunLoopDefaultMode);
  CFRelease(timer);
}

// Why a CFRunLoopTimer and never a main-queue block: Chromium's loop then
// runs inside this one callout for the rest of the process, and it has to be
// entered from a run-loop pass rather than a GCD block, or GPUI's main-queue
// work (every foreground task) would stop being serviced (Ghostex's note on
// this, 2026-10-17; the spike found the same).
void tb_cef_start_message_loop(void) {
  if (g_startPending || g_loopRunning) {
    return;
  }
  g_startPending = YES;
  tb_cef_schedule_start(0);
  CFRunLoopWakeUp(CFRunLoopGetMain());
}

// A page's native view placed in a GPUI layout slot. GPUI's coordinates are
// the window's, from the top left, in points; the parent view may not be
// flipped, so the y is turned over.
void tb_view_set_frame(void *nativeView, double x, double y, double width, double height) {
  NSView *view = (__bridge NSView *)nativeView;
  if (!view) {
    return;
  }
  // GPUI places it every frame; AppKit resizing it as well fights that.
  view.autoresizingMask = NSViewNotSizable;
  NSView *parent = view.superview;
  CGFloat nativeY = y;
  if (parent && !parent.isFlipped) {
    nativeY = NSHeight(parent.bounds) - y - height;
  }
  view.frame = NSMakeRect(x, nativeY, MAX(0.0, width), MAX(0.0, height));
}

void tb_view_set_hidden(void *nativeView, bool hidden) {
  NSView *view = (__bridge NSView *)nativeView;
  view.hidden = hidden;
}

// Moves a page's view into another window's view: its panel was dragged into
// a window of its own, or back. The local keeps the view alive between the
// two: its old superview held the only reference to it.
void tb_view_set_parent(void *nativeView, void *nativeParent) {
  NSView *view = (__bridge NSView *)nativeView;
  NSView *parent = (__bridge NSView *)nativeParent;
  if (!view || !parent || view.superview == parent) {
    return;
  }
  [view removeFromSuperview];
  [parent addSubview:view];
}

// Takes a page's view out of its window for good. Its parent held the only
// reference to it, so it goes, and with it the page: Chromium ends a child
// view's browser when the view goes (CefBrowserHostView's dealloc), without
// asking the window it was in to close, as CloseBrowser does.
void tb_view_remove(void *nativeView) {
  NSView *view = (__bridge NSView *)nativeView;
  [view removeFromSuperview];
}

// A person's clicks and keys in each watched page view, counted as AppKit
// hands them over. After a page sends something to the browser, the next
// needs a new one (panel_rules.rs's LeaveGate): Chromium leaves a click's
// activation with the page when the app makes no window for it, so the page
// could otherwise use one click for a page a second (Codex review, 12 Nov). Keys
// go to the view with the keyboard, clicks to the view under them, and a key
// held down counts once. A press from VoiceOver isn't counted, as AppKit
// never sees it. Weak keys: a page's view going takes its count with it.
static NSMapTable<NSView *, NSNumber *> *g_inputs;

static void tb_note_input(NSView *view) {
  if (!view) {
    return;
  }
  // Found first, counted after: the table can't change while it's walked.
  NSMutableArray<NSView *> *under = [NSMutableArray array];
  for (NSView *watched in g_inputs) {
    if ([view isDescendantOf:watched]) {
      [under addObject:watched];
    }
  }
  for (NSView *watched in under) {
    [g_inputs setObject:@([[g_inputs objectForKey:watched] unsignedLongLongValue] + 1) forKey:watched];
  }
}

void tb_view_watch_input(void *nativeView) {
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    g_inputs = [NSMapTable weakToStrongObjectsMapTable];
    NSEventMask mask = NSEventMaskLeftMouseDown | NSEventMaskRightMouseDown | NSEventMaskOtherMouseDown | NSEventMaskKeyDown;
    [NSEvent addLocalMonitorForEventsMatchingMask:mask handler:^NSEvent *(NSEvent *event) {
      NSWindow *window = event.window;
      if (event.type == NSEventTypeKeyDown) {
        if (event.isARepeat) {
          return event;
        }
        NSResponder *responder = window.firstResponder;
        tb_note_input([responder isKindOfClass:[NSView class]] ? (NSView *)responder : nil);
      } else if (window.contentView) {
        // hitTest takes a point in the view's superview's coordinates.
        NSView *content = window.contentView;
        NSView *frame = content.superview ?: content;
        NSPoint point = [frame convertPoint:event.locationInWindow fromView:nil];
        tb_note_input([content hitTest:point]);
      }
      return event;
    }];
  });
  NSView *view = (__bridge NSView *)nativeView;
  if (view && ![g_inputs objectForKey:view]) {
    [g_inputs setObject:@0 forKey:view];
  }
}

uint64_t tb_view_input_count(void *nativeView) {
  NSView *view = (__bridge NSView *)nativeView;
  return view ? [[g_inputs objectForKey:view] unsignedLongLongValue] : 0;
}

// A scripted app's stand-in for a person's click in the page (the specs'
// clicks come through DevTools, which AppKit never sees).
void tb_view_note_input(void *nativeView) {
  tb_note_input((__bridge NSView *)nativeView);
}

// Whether the page's view, or one inside it, has its window's keyboard.
bool tb_view_has_keyboard(void *nativeView) {
  NSView *view = (__bridge NSView *)nativeView;
  NSResponder *responder = view.window.firstResponder;
  return view && [responder isKindOfClass:[NSView class]] && [(NSView *)responder isDescendantOf:view];
}

// Who has the keyboard in the window GPUI's view `nativeView` fills: 0 that
// view, 1 a page's view inside it that's showing, 2 nobody or the window
// itself, 3 a page's view inside it that's hidden, 4 anything else. A page's
// view is a subview of GPUI's (tb_view_set_parent), so "inside it" is a page.
int tb_view_keyboard_holder(void *nativeView) {
  NSView *view = (__bridge NSView *)nativeView;
  NSWindow *window = view.window;
  if (!view || !window) {
    return 2;
  }
  NSResponder *responder = window.firstResponder;
  if (responder == view) {
    return 0;
  }
  if (!responder || responder == window) {
    return 2;
  }
  if ([responder isKindOfClass:[NSView class]] && [(NSView *)responder isDescendantOf:view]) {
    return [(NSView *)responder isHiddenOrHasHiddenAncestor] ? 3 : 1;
  }
  return 4;
}

// Gives GPUI's view its window's keyboard (web_panels.rs says when). It never
// takes it back by itself: it doesn't accept first responder, so a click on
// it leaves the keyboard with a page's view, and once that view's hidden,
// with the window, which beeps at every letter.
bool tb_view_take_keyboard(void *nativeView) {
  NSView *view = (__bridge NSView *)nativeView;
  NSWindow *window = view.window;
  if (!view || !window || window.firstResponder == view) {
    return false;
  }
  return [window makeFirstResponder:view];
}

// A scripted app's stand-in for typing `text` (UTF-8, letters and spaces):
// each character's key down and up handed to the window GPUI's view is in,
// which gives them to whoever has its keyboard, as AppKit does a person's
// keys. A script's own keystrokes go straight into GPUI and never meet AppKit,
// so they can't tell who has the keyboard.
void tb_view_send_keys(void *nativeView, const char *text) {
  NSView *view = (__bridge NSView *)nativeView;
  NSWindow *window = view.window;
  NSString *all = text ? [NSString stringWithUTF8String:text] : nil;
  if (!window || !all) {
    return;
  }
  // US keyboard key codes, which is all the input context reads besides the
  // characters themselves.
  static const unsigned short codes[26] = {0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6};
  for (NSUInteger i = 0; i < all.length; i++) {
    NSString *one = [all substringWithRange:NSMakeRange(i, 1)];
    unichar c = [one.lowercaseString characterAtIndex:0];
    unsigned short code = (c >= 'a' && c <= 'z') ? codes[c - 'a'] : 49;
    for (int down = 1; down >= 0; down--) {
      NSEvent *event = [NSEvent keyEventWithType:(down ? NSEventTypeKeyDown : NSEventTypeKeyUp)
                                        location:NSZeroPoint
                                   modifierFlags:0
                                       timestamp:[[NSProcessInfo processInfo] systemUptime]
                                    windowNumber:window.windowNumber
                                         context:nil
                                      characters:one
                     charactersIgnoringModifiers:one
                                       isARepeat:NO
                                         keyCode:code];
      [window sendEvent:event];
    }
  }
}

@interface TBMenuTarget : NSObject
@property(nonatomic) NSInteger chosen;
- (void)pick:(NSMenuItem *)item;
@end

@implementation TBMenuTarget
- (void)pick:(NSMenuItem *)item {
  self.chosen = item.tag;
}
@end

// A native menu at (x, y) in a window's view, in GPUI's coordinates (from the
// top left), for a menu that has to come up over a web page's view: GPUI draws
// its own menus inside the window, under the page. Blocks while it's open, as
// an NSMenu does, and returns the item chosen, or -1. An empty title is a
// separator; a symbol is an SF Symbol's name, shown beside the title as the
// Swift app's menus show theirs.
int tb_popup_menu(void *nativeView, double x, double y, const char *const *titles, const char *const *symbols, const char *const *tips, const bool *checked, int count) {
  NSView *view = (__bridge NSView *)nativeView;
  if (!view) {
    return -1;
  }
  NSMenu *menu = [[NSMenu alloc] initWithTitle:@""];
  menu.autoenablesItems = NO;
  TBMenuTarget *target = [TBMenuTarget new];
  target.chosen = -1;
  for (int i = 0; i < count; i++) {
    const char *title = titles[i];
    if (!title || !title[0]) {
      [menu addItem:[NSMenuItem separatorItem]];
      continue;
    }
    NSMenuItem *item = [[NSMenuItem alloc] initWithTitle:[NSString stringWithUTF8String:title] action:@selector(pick:) keyEquivalent:@""];
    item.target = target;
    item.tag = i;
    item.state = checked[i] ? NSControlStateValueOn : NSControlStateValueOff;
    if (symbols[i] && symbols[i][0]) {
      item.image = [NSImage imageWithSystemSymbolName:[NSString stringWithUTF8String:symbols[i]] accessibilityDescription:nil];
    }
    if (tips[i] && tips[i][0]) {
      item.toolTip = [NSString stringWithUTF8String:tips[i]];
    }
    [menu addItem:item];
  }
  CGFloat nativeY = view.isFlipped ? y : NSHeight(view.bounds) - y;
  [menu popUpMenuPositioningItem:nil atLocation:NSMakePoint(x, nativeY) inView:view];
  return (int)target.chosen;
}

// The app's bundle id, for the helpers' ids, or NULL when it isn't running
// from a bundle (a cargo build the specs start). The string is the bundle's
// and lives as long as it.
const char *tb_main_bundle_identifier(void) {
  NSString *identifier = [[NSBundle mainBundle] bundleIdentifier];
  return identifier ? identifier.UTF8String : NULL;
}
