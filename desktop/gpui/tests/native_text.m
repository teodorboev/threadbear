#import <AppKit/AppKit.h>
#import <objc/runtime.h>
#ifndef TB_SELECTION_SOURCE
#define TB_SELECTION_SOURCE "../native/text_selection.m"
#endif
#include TB_SELECTION_SOURCE

uint64_t tb_services_freeze(uint64_t window, uint64_t snapshot) { return 1; }
bool tb_services_return(uint64_t request, const unsigned char *words, size_t length) { return false; }
void tb_services_closed(uint64_t window) {}
void tb_services_discard(uint64_t request) {}

static NSUInteger contextual, direct;
static BOOL menuWasInstalled, copyHasTarget;
static NSMenuItem *shownCopy;
static NSWindow *testKeyWindow;
static BOOL throwPopup;
static BOOL available(id object, SEL selector) { return YES; }
static NSWindow *fixtureKeyWindow(id object, SEL selector) { return testKeyWindow; }
static void contextMenu(id object, SEL selector, NSMenu *menu, NSEvent *event, NSView *view) {
  contextual++;
  menuWasInstalled = view.menu == menu && event.type == NSEventTypeRightMouseDown;
  NSMenuItem *copy = menu.itemArray.firstObject;
  shownCopy = copy;
  copyHasTarget = copy.action == @selector(copy:) && [copy.target respondsToSelector:copy.action];
  if (throwPopup) [NSException raise:NSInternalInconsistencyException format:@"test popup failure"];
}
@interface TBWritingPlainView : NSView
@end
@implementation TBWritingPlainView
- (BOOL)acceptsFirstResponder { return YES; }
@end
@interface TBWritingActionView : TBWritingPlainView
@end
@implementation TBWritingActionView
- (void)showWritingTools:(id)sender { direct++; }
@end

int main(void) { @autoreleasepool {
  // No visible window, event posting or clipboard write: the menu call is
  // intercepted in this test process, and this CLI cannot become active.
  [NSApplication sharedApplication];
  [NSApp setActivationPolicy:NSApplicationActivationPolicyProhibited];
  if (@available(macOS 15.2, *)) {
    Method availability = class_getClassMethod(NSWritingToolsCoordinator.class, @selector(isWritingToolsAvailable));
    Method popup = class_getClassMethod(NSMenu.class, @selector(popUpContextMenu:withEvent:forView:));
    Method key = class_getInstanceMethod(NSApplication.class, @selector(keyWindow));
    IMP originalAvailability = method_setImplementation(availability, (IMP)available);
    IMP originalPopup = method_setImplementation(popup, (IMP)contextMenu);
    IMP originalKey = method_setImplementation(key, (IMP)fixtureKeyWindow);
    int result = 0;
    @try {
      NSWindow *window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,100,100)
        styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:NO];
      window.releasedWhenClosed = NO;
      NSView *view = [TBWritingPlainView new];
      window.contentView = view;
      [window makeFirstResponder:view];
      testKeyWindow = window;
      NSMenu *previous = [NSMenu new];
      view.menu = previous;
      const unsigned char words[] = "Selected words";
      tb_selection_update((__bridge void *)view, 1, 2, words, sizeof(words)-1, true);
      NSCAssert(![view respondsToSelector:@selector(showWritingTools:)], @"the plain custom view must exercise the unsupported-action case");
      NSCAssert(tb_selection_writing((void *)CFBridgingRetain(view)), @"the custom view must offer the supported native context menu");
      NSCAssert(contextual == 1 && direct == 0 && menuWasInstalled && copyHasTarget, @"Writing Tools must use AppKit's context-menu augmentation");
      NSCAssert(view.menu == previous, @"temporary menu must not replace the view owner's menu");
      TBSelectionProxy *proxy = objc_getAssociatedObject(view, &TBSelectionProxyKey);
      NSCAssert(shownCopy.target == proxy, @"Copy must target this view's checked requestor");
      testKeyWindow = nil;
      NSCAssert(![proxy validateMenuItem:shownCopy], @"an inactive window must not advertise a copy of stale words");
      NSCAssert(!tb_selection_writing((void *)CFBridgingRetain(view)) && contextual == 1, @"an inactive view must not show a menu that cannot serve its selection");
      testKeyWindow = window;
      NSCAssert([proxy validateMenuItem:shownCopy], @"the same Copy item is valid with its view's keyboard back");
      [window makeFirstResponder:nil];
      NSCAssert(![proxy validateMenuItem:shownCopy], @"another responder must not copy this view's selection");
      NSCAssert(!tb_selection_writing((void *)CFBridgingRetain(view)) && contextual == 1, @"another responder must not open this view's text menu");
      [window makeFirstResponder:view];
      throwPopup = YES;
      NSCAssert(!tb_selection_writing((void *)CFBridgingRetain(view)), @"a native exception must not cross the C boundary");
      NSCAssert(view.menu == previous, @"a failed popup must restore the previous menu");
      throwPopup = NO;
      window.contentView = [TBWritingActionView new];
      [window makeFirstResponder:window.contentView];
      tb_selection_update((__bridge void *)window.contentView, 3, 4, words, sizeof(words)-1, true);
      NSCAssert(tb_selection_writing((void *)CFBridgingRetain(window.contentView)), @"a view that actually implements the action may handle it");
      NSCAssert(direct == 1 && contextual == 2, @"the implemented action must dispatch once");
      [window close];
      puts("PASS: custom-view Writing Tools uses the native menu, restores ownership and respects implemented actions");
    } @catch (NSException *error) {
      fprintf(stderr, "FAIL: native text dispatch raised %s: %s\n", error.name.UTF8String, error.reason.UTF8String);
      result = 1;
    } @finally {
      method_setImplementation(availability, originalAvailability);
      method_setImplementation(popup, originalPopup);
      method_setImplementation(key, originalKey);
      testKeyWindow = nil;
    }
    return result;
  }
  puts("SKIP: Writing Tools needs macOS 15.2");
  return 0;
}}
