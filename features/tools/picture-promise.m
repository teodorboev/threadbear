// A real file-promise drag from an app of our own into the test app. Run only
// on Tinker Desk's desktop; no real Photos library or general clipboard.
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#include <stdio.h>
#include <unistd.h>
@interface PictureSource : NSView <NSFilePromiseProviderDelegate, NSDraggingSource>
@property(nonatomic, strong) NSData *data;
@property(nonatomic, strong) NSURL *file;
@end
@implementation PictureSource
- (void)drawRect:(NSRect)dirty { [[NSColor systemBlueColor] setFill]; NSRectFill(self.bounds); }
- (NSString *)filePromiseProvider:(NSFilePromiseProvider *)provider fileNameForType:(NSString *)type { return @"sample.png"; }
- (void)filePromiseProvider:(NSFilePromiseProvider *)provider writePromiseToURL:(NSURL *)url completionHandler:(void (^)(NSError *))completion {
  NSError *error = nil;
  [self.data writeToURL:url options:0 error:&error];
  completion(error);
}
- (NSOperationQueue *)operationQueueForFilePromiseProvider:(NSFilePromiseProvider *)provider {
  NSOperationQueue *queue = [NSOperationQueue new]; queue.maxConcurrentOperationCount = 1; return queue;
}
- (NSDragOperation)draggingSession:(NSDraggingSession *)session sourceOperationMaskForDraggingContext:(NSDraggingContext)context { return NSDragOperationCopy; }
- (void)mouseDown:(NSEvent *)event {
  fprintf(stderr, "Starting the file-promise drag\n");
  NSFilePromiseProvider *provider = [[NSFilePromiseProvider alloc] initWithFileType:@"public.png" delegate:self];
  NSDraggingItem *promise = [[NSDraggingItem alloc] initWithPasteboardWriter:provider];
  NSDraggingItem *normal = [[NSDraggingItem alloc] initWithPasteboardWriter:self.file];
  NSImage *image = [[NSImage alloc] initWithData:self.data];
  [promise setDraggingFrame:NSMakeRect(30, 30, 48, 48) contents:image];
  [normal setDraggingFrame:NSMakeRect(80, 30, 48, 48) contents:image];
  [self beginDraggingSessionWithItems:@[promise, normal] event:event source:self];
}
- (void)draggingSession:(NSDraggingSession *)session endedAtPoint:(NSPoint)point operation:(NSDragOperation)operation {
  fprintf(stderr, "Drag ended with operation %lu\n", (unsigned long)operation);
}
@end
static void post(CGEventType type, CGPoint point) {
  CGEventRef event = CGEventCreateMouseEvent(NULL, type, point, kCGMouseButtonLeft);
  CGEventPost(kCGHIDEventTap, event); CFRelease(event);
}
int main(int argc, const char **argv) {
  @autoreleasepool {
    if (argc == 4 && strcmp(argv[1], "--move") == 0) {
      if (!AXIsProcessTrusted()) return 5;
      post(kCGEventMouseMoved, CGPointMake(atof(argv[2]), atof(argv[3])));
      return 0;
    }
    // A separate command-line child posts the events under Desk Runner's
    // Accessibility grant; the foreground source app has an identity of its
    // own and must not ask for an Accessibility grant on the test desktop.
    if (argc == 7 && strcmp(argv[1], "--post") == 0) {
      if (!AXIsProcessTrusted()) { fprintf(stderr, "The Desk Runner event helper has no Accessibility access\n"); return 5; }
      CGPoint start = CGPointMake(atof(argv[2]), atof(argv[3]));
      CGPoint end = CGPointMake(atof(argv[4]), atof(argv[5]));
      pid_t target = atoi(argv[6]);
      pid_t over = 0;
      for (NSDictionary *info in CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID))) {
        CGRect frame;
        if ([info[(id)kCGWindowLayer] intValue] == 0 && CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)info[(id)kCGWindowBounds], &frame) && CGRectContainsPoint(frame, end)) {
          over = [info[(id)kCGWindowOwnerPID] intValue]; break;
        }
      }
      if (over != target) { fprintf(stderr, "The destination test window is covered\n"); return 6; }
      CGEventRef current = CGEventCreate(NULL); CGPoint restore = CGEventGetLocation(current); CFRelease(current);
      post(kCGEventMouseMoved, start); usleep(80000);
      post(kCGEventLeftMouseDown, start); usleep(80000);
      for (int i = 1; i <= 30; i++) { double fraction = i / 30.0; post(kCGEventLeftMouseDragged, CGPointMake(start.x + fraction * (end.x-start.x), start.y + fraction * (end.y-start.y))); usleep(20000); }
      usleep(150000); post(kCGEventLeftMouseUp, end);
      // Keep the pointer there until AppKit has finished negotiating the
      // promise. A move away during its nested run loop can abort the drop.
      printf("%.3f %.3f\n", restore.x, restore.y);
      return 0;
    }
    if (argc != 3) return 2;
    pid_t target = atoi(argv[2]);
    CGRect targetFrame = CGRectZero;
    for (NSDictionary *window in CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID))) {
      if ([window[(id)kCGWindowOwnerPID] intValue] == target && [window[(id)kCGWindowLayer] intValue] == 0) {
        CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)window[(id)kCGWindowBounds], &targetFrame); break;
      }
    }
    if (CGRectIsEmpty(targetFrame)) return 3;
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
    PictureSource *source = [[PictureSource alloc] initWithFrame:NSMakeRect(0, 0, 180, 140)];
    source.data = [NSData dataWithContentsOfFile:@(argv[1])];
    source.file = [NSURL fileURLWithPath:@(argv[1])];
    // Use the top of the usable screen area so an auto-hidden Dock appearing
    // at the bottom cannot cover the source either. This places the title bar
    // below the menu bar too, without guessing its height.
    NSRect visible = NSScreen.screens.firstObject.visibleFrame;
    NSWindow *window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 180, 140) styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
    [window setFrameTopLeftPoint:NSMakePoint(NSMinX(visible) + 30, NSMaxY(visible) - 30)];
    window.title = @"Threadbear picture test source";
    window.contentView = source;
    [window makeKeyAndOrderFront:nil]; [NSApp activateIgnoringOtherApps:YES];
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 500 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
      [[NSRunningApplication runningApplicationWithProcessIdentifier:target] activateWithOptions:NSApplicationActivateIgnoringOtherApps | NSApplicationActivateAllWindows];
      dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 300 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
      [window makeKeyAndOrderFront:nil];
      [NSRunningApplication.currentApplication activateWithOptions:NSApplicationActivateIgnoringOtherApps | NSApplicationActivateAllWindows];
      dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 500 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
      if (NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier != getpid()) { fprintf(stderr, "The source window is not in front (%s)\n", NSWorkspace.sharedWorkspace.frontmostApplication.localizedName.UTF8String); exit(4); }
      NSPoint origin = [window convertPointToScreen:NSMakePoint(80, 60)];
      CGPoint start = CGPointMake(origin.x, NSScreen.screens.firstObject.frame.size.height - origin.y);
      CGPoint end = CGPointMake(targetFrame.origin.x + 650, targetFrame.origin.y + 300);
      printf("%.3f %.3f %.3f %.3f %d\n", start.x, start.y, end.x, end.y, target); fflush(stdout);
      });
      });
    });
    [NSApp run];
  }
  return 0;
}
