// AppKit selection consumers for GPUI's drawn text. No hidden NSTextView:
// Services receives a frozen plain-text request, and replacement returns to
// Rust's original textarea/range check on the foreground executor.
#import <AppKit/AppKit.h>
#import <objc/runtime.h>
#include <stdbool.h>
#include <stdint.h>

extern uint64_t tb_services_freeze(uint64_t window, uint64_t snapshot);
extern bool tb_services_return(uint64_t request, const unsigned char *words, size_t length);
extern void tb_services_closed(uint64_t window);
extern void tb_services_discard(uint64_t request);
static const size_t TBSelectionLimit = 1024 * 1024;
static char TBSelectionProxyKey;

static BOOL tb_plain_type(NSPasteboardType type) {
  return [type isEqualToString:NSPasteboardTypeString] || [type isEqualToString:NSStringPboardType];
}

@interface TBSelectionRequest : NSResponder <NSServicesMenuRequestor>
@property(nonatomic, copy) NSString *words;
@property(nonatomic) uint64_t request;
@property(nonatomic) BOOL editable;
@property(nonatomic) BOOL delivered;
@end
@implementation TBSelectionRequest
- (void)dealloc {
  uint64_t request = _request;
  if (!_delivered) {
    if (NSThread.isMainThread) tb_services_discard(request);
    else dispatch_async(dispatch_get_main_queue(), ^{ tb_services_discard(request); });
  }
}
- (BOOL)writeSelectionToPasteboard:(NSPasteboard *)pasteboard types:(NSArray<NSPasteboardType> *)types {
  NSMutableArray<NSPasteboardType> *plain = [NSMutableArray new];
  for (NSPasteboardType type in types) if (tb_plain_type(type)) [plain addObject:type];
  if (!self.words.length || !plain.count) return NO;
  [pasteboard declareTypes:plain owner:nil];
  BOOL wrote = NO;
  for (NSPasteboardType type in plain) wrote |= [pasteboard setString:self.words forType:type];
  return wrote;
}
- (BOOL)readSelectionFromPasteboard:(NSPasteboard *)pasteboard {
  if (!NSThread.isMainThread || !self.editable || !self.request || self.delivered) return NO;
  NSString *words = [pasteboard stringForType:NSPasteboardTypeString] ?: [pasteboard stringForType:NSStringPboardType];
  if (!words || words.length > TBSelectionLimit) return NO;
  NSData *bytes = [words dataUsingEncoding:NSUTF8StringEncoding];
  BOOL accepted = bytes && bytes.length <= TBSelectionLimit && tb_services_return(self.request, bytes.bytes, bytes.length);
  if (accepted) self.delivered = YES;
  return accepted;
}
@end

@interface TBSelectionProxy : NSResponder <NSMenuItemValidation>
@property(nonatomic, weak) NSView *view;
@property(nonatomic) uint64_t window;
@property(nonatomic) uint64_t snapshot;
@property(nonatomic, copy) NSString *words;
@property(nonatomic) BOOL editable;
@end
@implementation TBSelectionProxy
- (BOOL)validateMenuItem:(NSMenuItem *)item {
  NSView *view = self.view;
  return item.action == @selector(copy:) && self.words.length && self.snapshot &&
    view.window == NSApp.keyWindow && view.window.firstResponder == view;
}
- (void)copy:(id)sender {
  id<NSServicesMenuRequestor> request = [self validRequestorForSendType:NSPasteboardTypeString returnType:nil];
  if ([request conformsToProtocol:@protocol(NSServicesMenuRequestor)]) {
    [request writeSelectionToPasteboard:NSPasteboard.generalPasteboard types:@[NSPasteboardTypeString]];
  }
}
- (void)dealloc {
  uint64_t window = _window;
  if (NSThread.isMainThread) tb_services_closed(window);
  else dispatch_async(dispatch_get_main_queue(), ^{ tb_services_closed(window); });
}
- (id)validRequestorForSendType:(NSPasteboardType)sendType returnType:(NSPasteboardType)returnType {
  NSView *view = self.view;
  // A browser or native field owns its own selection and responder chain.
  if (NSThread.isMainThread && view.window == NSApp.keyWindow && view.window.firstResponder == view &&
      tb_plain_type(sendType) && (!returnType || (self.editable && tb_plain_type(returnType))) && self.words.length && self.snapshot) {
    uint64_t request = tb_services_freeze(self.window, self.snapshot);
    if (request) {
      TBSelectionRequest *frozen = [TBSelectionRequest new];
      frozen.words = self.words;
      frozen.request = request;
      frozen.editable = returnType && self.editable;
      return frozen;
    }
  }
  return [super validRequestorForSendType:sendType returnType:returnType];
}
@end

// Main-thread calls from a retained, live AppKit view. The associated proxy
// lives with the view; its weak view and unowned responder links make no
// window retain cycle. Reinsert only if another owner removed it entirely.
void tb_selection_update(void *nativeView, uint64_t window, uint64_t snapshot, const unsigned char *words, size_t length, bool editable) {
  if (!NSThread.isMainThread || !nativeView) return;
  NSView *view = (__bridge NSView *)nativeView;
  TBSelectionProxy *proxy = objc_getAssociatedObject(view, &TBSelectionProxyKey);
  if (!proxy) {
    proxy = [TBSelectionProxy new];
    proxy.view = view;
    proxy.window = window;
    objc_setAssociatedObject(view, &TBSelectionProxyKey, proxy, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  }
  BOOL present = NO;
  NSResponder *at = view.nextResponder;
  for (int n = 0; at && n < 128; n++, at = at.nextResponder) if (at == proxy) { present = YES; break; }
  if (!present) { proxy.nextResponder = view.nextResponder; view.nextResponder = proxy; }
  proxy.editable = editable;
  if (proxy.snapshot == snapshot) return;
  NSString *selection = words && length <= TBSelectionLimit ? [[NSString alloc] initWithBytes:words length:length encoding:NSUTF8StringEncoding] : nil;
  proxy.snapshot = selection.length ? snapshot : 0;
  proxy.words = selection;
}

// Own this temporary retain across the deferred AppKit operation. Looking
// up a definition can open a system popover and must run outside GPUI's lease.
void *tb_selection_retain(void *view) { return view ? (void *)CFBridgingRetain((__bridge NSView *)view) : NULL; }
void tb_selection_lookup(void *retained, const unsigned char *words, size_t length, double x, double y) {
  NSView *view = CFBridgingRelease(retained);
  if (!NSThread.isMainThread || !view.window || !words || length == 0 || length > 4096) return;
  NSString *text = [[NSString alloc] initWithBytes:words length:length encoding:NSUTF8StringEncoding];
  if (!text.length) return;
  NSPoint point = NSMakePoint(x, view.isFlipped ? y : NSHeight(view.bounds) - y);
  [view showDefinitionForAttributedString:[[NSAttributedString alloc] initWithString:text] atPoint:point];
}

// Show the real Services menu after the GPUI context menu has closed. Its
// contents and enabled states belong to macOS, using the requestor above.
bool tb_selection_services(void *retained, double x, double y) {
  NSView *view = CFBridgingRelease(retained);
  if (!NSThread.isMainThread || !view.window) return false;
  NSMenu *menu = NSApp.servicesMenu;
  [menu update];
  if (!menu || menu.numberOfItems == 0) return false;
  NSPoint point = NSMakePoint(x, view.isFlipped ? y : NSHeight(view.bounds) - y);
  [menu popUpMenuPositioningItem:nil atLocation:point inView:view];
  return true;
}

// AppKit adds its pasteboard-backed Writing Tools items to a view's native
// contextual menu. Merely declaring showWritingTools: on NSResponder does
// not implement that action on a custom NSView (GPUIView raises an exception).
// Use the public context-menu path and the existing Services requestor:
// https://developer.apple.com/documentation/appkit/supporting-writing-tools-via-the-pasteboard
bool tb_selection_writing_available(void) {
  if (@available(macOS 15.2, *)) return NSWritingToolsCoordinator.isWritingToolsAvailable;
  return false;
}
bool tb_selection_writing(void *retained) {
  NSView *view = CFBridgingRelease(retained);
  if (!NSThread.isMainThread || !view.window || !tb_selection_writing_available()) return false;
  TBSelectionProxy *proxy = objc_getAssociatedObject(view, &TBSelectionProxyKey);
  if (!proxy.words.length || !proxy.snapshot || view.window != NSApp.keyWindow || view.window.firstResponder != view) return false;
  @try {
    if ([view respondsToSelector:@selector(showWritingTools:)]) {
      [view showWritingTools:nil];
      return true;
    }
    NSMenu *menu = [NSMenu new];
    NSMenuItem *copy = [[NSMenuItem alloc] initWithTitle:@"Copy" action:@selector(copy:) keyEquivalent:@""];
    copy.target = proxy;
    [menu addItem:copy];
    NSPoint location = view.window.mouseLocationOutsideOfEventStream;
    NSPoint inView = [view convertPoint:location fromView:nil];
    if (!NSPointInRect(inView, view.bounds)) {
      location = [view convertPoint:NSMakePoint(NSMidX(view.bounds), NSMidY(view.bounds)) toView:nil];
    }
    NSEvent *event = [NSEvent mouseEventWithType:NSEventTypeRightMouseDown location:location modifierFlags:0
      timestamp:NSProcessInfo.processInfo.systemUptime windowNumber:view.window.windowNumber context:nil
      eventNumber:0 clickCount:1 pressure:0];
    NSMenu *previous = view.menu;
    view.menu = menu;
    @try {
      [NSMenu popUpContextMenu:menu withEvent:event forView:view];
    } @finally {
      if (view.menu == menu) view.menu = previous;
    }
    return true;
  } @catch (NSException *error) {
    // An AppKit exception must never unwind across Rust's C ABI. Neither
    // the selected words nor a provider's exception text goes in the log.
    NSLog(@"Threadbear: Writing Tools could not open.");
    return false;
  }
}

bool tb_selection_translate_available(void) { if (@available(macOS 14.4, *)) return true; return false; }

static char TBSelectionShareKey;
void tb_selection_share(void *retained, const unsigned char *words, size_t length, double x, double y) {
  NSView *view = CFBridgingRelease(retained);
  if (!NSThread.isMainThread || !view.window || !words || !length || length > TBSelectionLimit) return;
  NSString *text = [[NSString alloc] initWithBytes:words length:length encoding:NSUTF8StringEncoding];
  if (!text.length) return;
  NSSharingServicePicker *picker = [[NSSharingServicePicker alloc] initWithItems:@[text]];
  // At most one picker per GPUI view, released when another is shown or the
  // view closes. No file URL or agent-provided object reaches a share service.
  objc_setAssociatedObject(view, &TBSelectionShareKey, picker, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  NSRect anchor = NSMakeRect(x, view.isFlipped ? y : NSHeight(view.bounds) - y, 1, 1);
  [picker showRelativeToRect:anchor ofView:view preferredEdge:NSMinYEdge];
}

#ifdef TB_TEXT_TEST
// Debug-only integration: use the real AppKit responder and a private
// pasteboard. No system service is invoked and the general board is untouched.
static NSMutableDictionary<NSNumber *, TBSelectionRequest *> *TBTestRequests;
uint64_t tb_selection_test_begin(void *retained, bool returning, unsigned char *out, size_t capacity, size_t *length) {
  NSView *view = CFBridgingRelease(retained);
  if (!NSThread.isMainThread || !view.window || !out || !length) return 0;
  id requestor = [view validRequestorForSendType:NSPasteboardTypeString returnType:returning ? NSPasteboardTypeString : nil];
  if (![requestor isKindOfClass:TBSelectionRequest.class]) return 0;
  TBSelectionRequest *request = requestor;
  NSPasteboard *board = [NSPasteboard pasteboardWithUniqueName];
  BOOL written = [request writeSelectionToPasteboard:board types:@[NSPasteboardTypeString]];
  NSData *bytes = [[board stringForType:NSPasteboardTypeString] dataUsingEncoding:NSUTF8StringEncoding];
  [board releaseGlobally];
  if (!written || !bytes || bytes.length > capacity) return 0;
  memcpy(out, bytes.bytes, bytes.length); *length = bytes.length;
  if (!TBTestRequests) TBTestRequests = [NSMutableDictionary new];
  TBTestRequests[@(request.request)] = request;
  return request.request;
}
bool tb_selection_test_return(uint64_t request, const unsigned char *words, size_t length) {
  if (!NSThread.isMainThread || !words || length > TBSelectionLimit) return false;
  NSString *text = [[NSString alloc] initWithBytes:words length:length encoding:NSUTF8StringEncoding];
  if (!text) return false;
  TBSelectionRequest *frozen = TBTestRequests[@(request)];
  if (!frozen) return false;
  NSPasteboard *board = [NSPasteboard pasteboardWithUniqueName];
  [board declareTypes:@[NSPasteboardTypeString] owner:nil];
  [board setString:text forType:NSPasteboardTypeString];
  BOOL accepted = [frozen readSelectionFromPasteboard:board];
  [board releaseGlobally];
  return accepted;
}
void tb_selection_test_discard(uint64_t request) { [TBTestRequests removeObjectForKey:@(request)]; }
size_t tb_selection_test_windows(unsigned char *out, size_t capacity) {
  if (!NSThread.isMainThread || !out) return 0;
  NSMutableArray *windows = [NSMutableArray new];
  for (NSWindow *window in NSApp.windows) {
    if (window.visible) [windows addObject:@{@"number":@(window.windowNumber), @"class":NSStringFromClass(window.class), @"key":@(window.keyWindow)}];
  }
  NSData *json = [NSJSONSerialization dataWithJSONObject:windows options:0 error:nil];
  if (!json || json.length > capacity) return 0;
  memcpy(out,json.bytes,json.length); return json.length;
}
void tb_selection_test_dismiss(void) {
  NSWindow *window=NSApp.keyWindow;
  if(!window)return;
  for(int down=1;down>=0;down--){
    NSEvent *event=[NSEvent keyEventWithType:down?NSEventTypeKeyDown:NSEventTypeKeyUp location:NSZeroPoint modifierFlags:0 timestamp:NSProcessInfo.processInfo.systemUptime windowNumber:window.windowNumber context:nil characters:@"\033" charactersIgnoringModifiers:@"\033" isARepeat:NO keyCode:53];
    [NSApp sendEvent:event];
  }
}
#endif
