// GPUI's Mac platform reads only NSFilenamesPboardType for a drag, and drops
// HEIC and Photos' per-item representations on paste. Keep AppKit's original
// pasteboard until Submit, then let the composer own its bytes before returning.
#import <AppKit/AppKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#import <objc/runtime.h>
#include <stdbool.h>

static NSArray<NSString *> *attachmentExtensions(void) {
  return @[@"png", @"jpg", @"jpeg", @"gif", @"webp", @"heic", @"heif", @"tif", @"tiff", @"bmp"];
}

// A UTType can include more suffixes than media.import accepts. Disable
// those too, so every selectable file is one the composer can attach.
@interface TBImagePanelDelegate : NSObject <NSOpenSavePanelDelegate>
@end
@implementation TBImagePanelDelegate
- (BOOL)panel:(id)sender shouldEnableURL:(NSURL *)url {
  NSNumber *directory;
  [url getResourceValue:&directory forKey:NSURLIsDirectoryKey error:nil];
  return directory.boolValue || [attachmentExtensions() containsObject:url.pathExtension.lowercaseString];
}
@end
static char imagePanelDelegateKey;

// This sheet belongs to the composer window, and completes once on Add or
// Cancel. It never accepts a document that would become a generic file chip.
void tb_composer_choose_images(void *view, void (*finished)(void *, const char *const *, size_t), void *context) {
  NSOpenPanel *panel = [NSOpenPanel openPanel];
  panel.canChooseFiles = YES;
  panel.canChooseDirectories = NO;
  panel.allowsMultipleSelection = YES;
  // Match the image formats media.import can read; SVGs cannot be attached.
  NSMutableArray<UTType *> *types = [NSMutableArray array];
  for (NSString *extension in attachmentExtensions()) {
    UTType *type = [UTType typeWithFilenameExtension:extension];
    if (type && [type conformsToType:UTTypeImage]) [types addObject:type];
  }
  TBImagePanelDelegate *delegate = [TBImagePanelDelegate new];
  panel.delegate = delegate;
  // NSOpenPanel's delegate is weak; the panel owns this one for the sheet.
  objc_setAssociatedObject(panel, &imagePanelDelegateKey, delegate, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  panel.allowedContentTypes = types;
  panel.allowsOtherFileTypes = NO;
  panel.prompt = @"Add";
  void (^complete)(NSModalResponse) = ^(NSModalResponse result) {
    if (result != NSModalResponseOK) { finished(context, NULL, 0); return; }
    NSArray<NSURL *> *urls = panel.URLs;
    const char **names = calloc(urls.count, sizeof(char *));
    for (NSUInteger i = 0; i < urls.count; i++) names[i] = urls[i].path.UTF8String;
    finished(context, names, urls.count);
    free(names);
  };
  NSWindow *window = ((__bridge NSView *)view).window;
  if (window) [panel beginSheetModalForWindow:window completionHandler:complete];
  else [panel beginWithCompletionHandler:complete];
}

typedef bool (*TBMediaSink)(void *, int, const char *, const void *, size_t);
// Only valid during the synchronous sink call. Rust copies these bytes
// before returning, so a later file-open failure never rereads a pasteboard.
typedef struct {
  const char *extension;
  const void *bytes;
  size_t length;
  const void *promise;
} TBImageFallback;
static NSPasteboard *dropBoard;
static bool dropRead;
#ifdef TB_COMPOSER_AUTOMATION
static NSPasteboard *fixtureBoard;
static id fixtureLegacy;
static bool fixturePairedPromise;
static NSString *fixtureProtectedPath;
static void (*fixtureEnded)(void);
#endif
static NSArray<NSString *> *imageTypes(void) {
  return @[@"public.png", @"public.heic", @"public.heif", @"public.jpeg", @"com.compuserve.gif", @"org.webmproject.webp", @"public.tiff"];
}
static NSString *extensionForType(NSString *type) {
  if ([type isEqual:@"public.png"]) return @"png";
  if ([type isEqual:@"public.heic"]) return @"heic";
  if ([type isEqual:@"public.heif"]) return @"heif";
  if ([type isEqual:@"public.jpeg"]) return @"jpeg";
  if ([type isEqual:@"com.compuserve.gif"]) return @"gif";
  if ([type isEqual:@"org.webmproject.webp"]) return @"webp";
  return @"png";
}
static NSData *imageBytes(NSPasteboardItem *item, bool allowTIFF, NSString **extension) {
  for (NSString *type in imageTypes()) {
    if (![item.types containsObject:type]) continue;
    // Finder's TIFF beside a file URL may just be its icon. Photos' TIFF
    // is the copied photo; a standalone TIFF paste is also an image.
    if (!allowTIFF && [type isEqual:NSPasteboardTypeTIFF]) continue;
    NSData *data = [item dataForType:type];
    if ([type isEqual:NSPasteboardTypeTIFF]) {
      data = [[NSBitmapImageRep imageRepWithData:data] representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
    }
    if (data.length) { *extension = extensionForType(type); return data; }
  }
  return nil;
}
static bool pictureName(NSString *name) {
  return [@[@"png", @"jpg", @"jpeg", @"gif", @"webp", @"heic", @"heif", @"tif", @"tiff", @"bmp"] containsObject:name.pathExtension.lowercaseString];
}
static bool webURL(NSString *text) {
  NSURL *url = text ? [NSURL URLWithString:text] : nil;
  return [@[@"http", @"https"] containsObject:url.scheme.lowercaseString];
}
// composer_media.rs: whether the HTML is an <img> of the URL itself.
extern bool tb_drag_html_shows_image(const void *html, size_t htmlLength, const void *url, size_t urlLength);
// html_tags::DRAG_HTML_BYTES, in UTF-16 units: checked before the UTF-8 copy,
// and all a drag's items' HTML together (`budget`), so many items of HTML
// that doesn't match can't add up (Codex's review). A URL longer than any
// real picture's isn't one.
#define TB_DRAG_HTML_UNITS (256 * 1024)
#define TB_DRAG_ITEMS 16
#define TB_DRAG_URL_UNITS (16 * 1024)
static bool imageURL(NSPasteboardItem *item, NSUInteger *budget) {
  NSString *text = [item stringForType:NSPasteboardTypeURL];
  if (text.length > TB_DRAG_URL_UNITS || !webURL(text)) return false;
  if (pictureName([NSURL URLWithString:text].path)) return true;
  // Chrome/Safari can offer extensionless image URLs with their <img>
  // markup. An ordinary link must never be fetched as a picture: a GET
  // might consume a sign-in or unsubscribe link.
  NSString *html = [item stringForType:NSPasteboardTypeHTML];
  // This runs in draggingEntered, on the main thread, on HTML any web page
  // can put in a drag. A regex here backtracked: 40 KB of `<img ` held the
  // app five seconds (Daybreak 120, 13 Nov 2026). Now it's a size check
  // before any copy, then one pass in Rust (html_tags.rs).
  if (!html) return false;
  if (html.length > *budget) { *budget = 0; return false; }
  *budget -= html.length;
  NSData *markup = [html dataUsingEncoding:NSUTF8StringEncoding];
  NSData *want = [text dataUsingEncoding:NSUTF8StringEncoding];
  return markup && want && tb_drag_html_shows_image(markup.bytes, markup.length, want.bytes, want.length);
}
static bool picturePromise(NSFilePromiseReceiver *promise) {
  for (NSString *type in promise.fileTypes) {
    if ([[UTType typeWithIdentifier:type] conformsToType:UTTypeImage] || pictureName([@"photo." stringByAppendingString:type])) return true;
  }
  return false;
}
static bool readBoard(NSPasteboard *board, bool drag, TBMediaSink sink, void *context) {
  if (!board) return false;
  NSArray<NSPasteboardItem *> *items = [board.pasteboardItems copy];
  bool hasPromises = [board canReadObjectForClasses:@[NSFilePromiseReceiver.class] options:nil];
  bool hasFiles = [board canReadObjectForClasses:@[NSURL.class] options:@{NSPasteboardURLReadingFileURLsOnlyKey:@YES}];
  // Rich text with an image representation stays text. A browser's drag can
  // carry both a URL/title and the actual image; drops prefer the image.
  if (!drag && !hasFiles && !hasPromises && [board.types containsObject:NSPasteboardTypeString]) return false;
  bool taken = false;
  NSArray<NSFilePromiseReceiver *> *promises = hasPromises ? [[board readObjectsForClasses:@[NSFilePromiseReceiver.class] options:nil] copy] : @[];
  NSUInteger promiseIndex = 0;
  NSSet *promiseTypes = [NSSet setWithArray:[NSFilePromiseReceiver readableTypesForPasteboard:board]];
  NSUInteger htmlBudget = TB_DRAG_HTML_UNITS;
  for (NSPasteboardItem *item in items) {
    // readObjects returns receivers in pasteboard-item order. Advance even
    // when this item's file or image bytes win, so alternatives stay paired
    // with their own source, including two unrelated files named alike.
    NSFilePromiseReceiver *promise = nil;
    if ([[NSSet setWithArray:item.types] intersectsSet:promiseTypes] && promiseIndex < promises.count) {
      promise = promises[promiseIndex++];
    }
#ifdef TB_COMPOSER_AUTOMATION
    if (fixturePairedPromise && item == items.firstObject) promise = fixtureLegacy;
#endif
    NSString *fileString = [item stringForType:NSPasteboardTypeFileURL];
    NSURL *file = fileString ? [NSURL URLWithString:fileString] : nil;
    bool photos = [file.path.lowercaseString containsString:@".photoslibrary/"];
    // A Photos video is a file path too, never a potentially huge image read.
    if (file.isFileURL && !pictureName(file.path)) {
      sink(context, 0, file.path.UTF8String, NULL, 0);
      taken = true;
      continue;
    }
    bool denied = false;
#ifdef TB_COMPOSER_AUTOMATION
    // access(2) may say yes even though TCC denies an actual open.
    denied = [file.path isEqual:fixtureProtectedPath];
#endif
    NSString *imageExtension = nil;
    // Intentionally obtain provider bytes before the drop ends, even when
    // a file URL is offered. A lazy provider can delay this pasteboard call;
    // retaining only its item would lose the fallback if it clears the board.
    // The following C struct borrows NSData's interior buffer through sink.
    __attribute__((objc_precise_lifetime)) NSData *imageData = imageBytes(item, !file.isFileURL || photos, &imageExtension);
    // Keep the canonical file's bytes, not its icon/preview. Only temporary
    // sources need an immediate snapshot; ordinary opens run on a worker.
    // Capture alternate image data now, in case the app's later open fails.
    // Rust begins any fallback promise now, during the native callback.
    // Its owned receiver can wait beside the file's background open.
    TBImageFallback fallback = {imageExtension.UTF8String, imageData.bytes, imageData.length, picturePromise(promise) ? (__bridge const void *)promise : NULL};
    if (file.isFileURL && !photos && !denied && sink(context, 5, file.path.UTF8String, &fallback, sizeof(fallback))) {
      taken = true;
      continue;
    }
    bool saved = false;
    if (imageData.length) {
      sink(context, 1, imageExtension.UTF8String, imageData.bytes, imageData.length);
      saved = taken = true;
    }
    if (!saved && picturePromise(promise)) {
      sink(context, 3, NULL, (__bridge const void *)promise, 0);
      taken = true;
      continue;
    }
    if (!saved && file.isFileURL && !photos) {
      // If private import could not start, let the runner report the file's
      // error instead of substituting an icon or dropping this bundle item.
      sink(context, 0, file.path.UTF8String, NULL, 0);
      taken = true;
      continue;
    }
    if (!saved && file.isFileURL && photos) {
      // With no alternate representation, read in this app, which has the
      // drag/paste grant; never ask the runner to open a Photos library file.
      NSNumber *size;
      [file getResourceValue:&size forKey:NSURLFileSizeKey error:nil];
      NSData *data = size && size.unsignedLongLongValue <= 100 * 1024 * 1024 ? [NSData dataWithContentsOfURL:file] : nil;
      if (data.length) { sink(context, 1, file.pathExtension.UTF8String, data.bytes, data.length); saved = taken = true; }
    }
    if (!saved && drag && !file.isFileURL) {
      NSString *url = [item stringForType:NSPasteboardTypeURL];
      if (imageURL(item, &htmlBudget)) {
        sink(context, 2, url.UTF8String, NULL, 0);
        taken = true;
      }
    }
  }
  if (!items.count) {
    for (NSFilePromiseReceiver *promise in promises) {
      if (picturePromise(promise)) { sink(context, 3, NULL, (__bridge const void *)promise, 0); taken = true; }
    }
  }
  // Older Finder writers can provide just the legacy list, with no items.
  if (!taken) {
    for (NSString *path in [board propertyListForType:NSFilenamesPboardType]) {
      sink(context, 0, path.UTF8String, NULL, 0);
      taken = true;
    }
  }
  return taken;
}
bool tb_composer_read(bool drag, TBMediaSink sink, void *context) {
#ifdef TB_COMPOSER_AUTOMATION
  if (!drag && fixtureBoard) {
    bool taken = readBoard(fixtureBoard, false, sink, context);
    if (fixtureLegacy && !fixturePairedPromise) { sink(context, 3, NULL, (__bridge const void *)fixtureLegacy, 0); taken = true; }
    return taken;
  }
#endif
  if (drag && dropBoard && !dropRead) {
    dropRead = true;
    readBoard(dropBoard, true, sink, context);
    return true; // Never fall back to the adapter's marker on a failed read.
  }
  return !drag && readBoard(NSPasteboard.generalPasteboard, false, sink, context);
}

// The destination is a fresh 0700 folder shared by this drop's receivers. The
// promise may complete after the drop; the runner waits for this callback.
void tb_composer_promise(const void *object, const char *destination, TBMediaSink sink, void *context) {
  NSFilePromiseReceiver *promise = (__bridge NSFilePromiseReceiver *)object;
  NSOperationQueue *queue = [NSOperationQueue new];
  queue.maxConcurrentOperationCount = 1;
  __block bool delivered = false;
  __block NSUInteger expected = 1, received = 0;
  queue.suspended = YES;
  NSURL *folder = [NSURL fileURLWithPath:@(destination) isDirectory:YES];
  // Start receipt during the drop, on AppKit's thread. Deferring this call
  // lets the drag pasteboard lose its items and can crash inside AppKit's
  // type cache. Only delivery and file snapshots run on the reader queue.
  @try {
  [promise receivePromisedFilesAtDestination:folder
    options:@{} operationQueue:queue reader:^(NSURL *file, NSError *error) {
      if (delivered) return;
      @autoreleasepool {
        NSNumber *size = nil, *regular = nil;
        if (!error) {
          [file getResourceValue:&size forKey:NSURLFileSizeKey error:nil];
          [file getResourceValue:&regular forKey:NSURLIsRegularFileKey error:nil];
        }
        if (!error && regular.boolValue && size && size.unsignedLongLongValue <= 100 * 1024 * 1024 && pictureName(file.path)) {
          // Rust snapshots the open file in this reader callback, preserving
          // HEIC/JPEG bytes and the real name instead of making a huge PNG.
          sink(context, 6, file.path.UTF8String, NULL, 0);
        } else { sink(context, -1, NULL, NULL, 0); }
      }
      received++;
      if (received >= expected) {
        delivered = true;
        sink(context, 4, NULL, NULL, 0);
      }
    }];
    // Names become available only after calling in the promise. Hold its
    // reader queue until this count is known, including legacy bundles.
    expected = MAX((NSUInteger)1, promise.fileNames.count);
    queue.suspended = NO;
  } @catch (NSException *exception) {
    delivered = true;
    [queue cancelAllOperations]; queue.suspended = NO;
    sink(context, -1, NULL, NULL, 0);
    sink(context, 4, NULL, NULL, 0);
  }
}

// Present a filenames flavour to GPUI without changing the source app's
// pasteboard. Only Entered uses this adapter; Submit reads the real board.
@interface TBComposerDrag : NSObject
@property(nonatomic, strong) id<NSDraggingInfo> original;
@property(nonatomic, strong) NSPasteboard *board;
@end
@implementation TBComposerDrag
- (NSPasteboard *)draggingPasteboard { return self.board; }
- (id)draggingSource { return self.original.draggingSource; }
- (NSPoint)draggingLocation { return self.original.draggingLocation; }
@end
static NSDragOperation (*originalEntered)(id, SEL, id<NSDraggingInfo>);
static NSDragOperation (*originalUpdated)(id, SEL, id<NSDraggingInfo>);
static char acceptedKey;
static BOOL (*originalDrop)(id, SEL, id<NSDraggingInfo>);
static NSDragOperation enterWithTypes(id window, SEL sel, id<NSDraggingInfo> info) {
  NSPasteboard *source = info.draggingPasteboard;
  NSArray *legacy = [source propertyListForType:NSFilenamesPboardType];
  if (legacy.count) return originalEntered(window, sel, info);
  NSMutableArray *paths = [NSMutableArray new];
  for (NSURL *url in [source readObjectsForClasses:@[NSURL.class] options:@{NSPasteboardURLReadingFileURLsOnlyKey:@YES}]) {
    if (url.isFileURL) [paths addObject:url.path];
  }
  if (!paths.count) {
    NSMutableArray *types = [imageTypes() mutableCopy];
    bool hasPicture = [source availableTypeFromArray:types] != nil;
    bool hasURL = false;
    NSUInteger budget = TB_DRAG_HTML_UNITS, seen = 0;
    for (NSPasteboardItem *item in source.pasteboardItems) {
      if (++seen > TB_DRAG_ITEMS) break;
      if (imageURL(item, &budget)) { hasURL = true; break; }
    }
    bool hasPromise = false;
    if ([source canReadObjectForClasses:@[NSFilePromiseReceiver.class] options:nil]) {
      for (NSFilePromiseReceiver *promise in [source readObjectsForClasses:@[NSFilePromiseReceiver.class] options:nil]) {
        if (picturePromise(promise)) hasPromise = true;
      }
    }
    if (!hasPicture && !hasURL && !hasPromise) return originalEntered(window, sel, info);
    // GPUI needs an ExternalPaths drag to route and highlight the drop.
    // This marker is never sent to the runner: Submit reads source instead.
    [paths addObject:@"/threadbear-picture-drop.png"];
  }
  TBComposerDrag *adapter = [TBComposerDrag new];
  adapter.original = info;
  adapter.board = [NSPasteboard pasteboardWithUniqueName];
  [adapter.board setPropertyList:paths forType:NSFilenamesPboardType];
  NSDragOperation result = originalEntered(window, sel, (id)adapter);
  [adapter.board releaseGlobally];
  return result;
}
static NSDragOperation entered(id window, SEL sel, id<NSDraggingInfo> info) {
  NSDragOperation result = enterWithTypes(window, sel, info);
  objc_setAssociatedObject(window, &acceptedKey, @(result != NSDragOperationNone), OBJC_ASSOCIATION_RETAIN_NONATOMIC);
  return result;
}
static NSDragOperation updated(id window, SEL sel, id<NSDraggingInfo> info) {
  if (![objc_getAssociatedObject(window, &acceptedKey) boolValue]) return NSDragOperationNone;
  return originalUpdated(window, sel, info);
}
static BOOL dropped(id window, SEL sel, id<NSDraggingInfo> info) {
  if (![objc_getAssociatedObject(window, &acceptedKey) boolValue]) return NO;
  NSPasteboard *previous = dropBoard;
  bool previousRead = dropRead;
  dropBoard = info.draggingPasteboard;
  dropRead = false;
  @try { return originalDrop(window, sel, info); }
  @finally { dropBoard = previous; dropRead = previousRead; }
}
void tb_composer_install(void *viewPointer) {
  NSView *view = (__bridge NSView *)viewPointer;
  NSWindow *window = view.window;
  if (!window) return;
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    Class cls = window.class;
    Method enter = class_getInstanceMethod(cls, @selector(draggingEntered:));
    Method update = class_getInstanceMethod(cls, @selector(draggingUpdated:));
    Method drop = class_getInstanceMethod(cls, @selector(performDragOperation:));
    originalEntered = (void *)method_setImplementation(enter, (IMP)entered);
    originalUpdated = (void *)method_setImplementation(update, (IMP)updated);
    originalDrop = (void *)method_setImplementation(drop, (IMP)dropped);
  });
  NSMutableArray *types = [imageTypes() mutableCopy];
  [types addObjectsFromArray:@[NSFilenamesPboardType, NSPasteboardTypeFileURL, NSPasteboardTypeURL]];
  [types addObjectsFromArray:NSFilePromiseReceiver.readableDraggedTypes];
  [window registerForDraggedTypes:types];
}

#ifdef TB_COMPOSER_AUTOMATION
// Test pasteboards are isolated from the desktop's general clipboard. They
// exercise the same reader as a real paste/drop, including multiple items.
void tb_composer_fixture_end(void) {
  [fixtureBoard releaseGlobally];
  fixtureBoard = nil;
  fixtureLegacy = nil;
  fixturePairedPromise = false;
  fixtureProtectedPath = nil;
  if (fixtureEnded) fixtureEnded();
}
// Contract fixture for a legacy receiver that bundles two files. Real
// NSFilePromiseProvider emits one per item (tested by picture-promise.m).
// Like AppKit, names are empty until receivePromisedFiles is called.
@interface TBLegacyPictures : NSObject
@property(nonatomic, strong) NSData *data;
@property(nonatomic) bool called;
@property(nonatomic) bool failFirst;
@property(nonatomic) bool withdrawn;
@end
@implementation TBLegacyPictures
- (NSArray *)fileTypes { return @[@"public.png"]; }
- (NSArray *)fileNames { return !self.called ? @[] : self.failFirst ? @[@"missing.png", @"one.png", @"two.png"] : @[@"one.png", @"two.png"]; }
- (void)receivePromisedFilesAtDestination:(NSURL *)destination options:(NSDictionary *)options operationQueue:(NSOperationQueue *)queue reader:(void (^)(NSURL *, NSError *))reader {
  if (self.withdrawn) [NSException raise:NSRangeException format:@"The source removed its promise items"];
  self.called = true;
  for (NSString *name in self.fileNames) {
    [queue addOperationWithBlock:^{
      [NSThread sleepForTimeInterval:0.15];
      if ([name isEqual:@"missing.png"]) { reader(nil, [NSError errorWithDomain:NSCocoaErrorDomain code:NSFileReadNoSuchFileError userInfo:nil]); return; }
      NSURL *file = [destination URLByAppendingPathComponent:name];
      NSError *error = nil;
      [self.data writeToURL:file options:0 error:&error];
      reader(file, error);
    }];
  }
}
@end
void tb_composer_fixture_begin(const char *kindText, const char *pathText, void (*ended)(void)) {
  tb_composer_fixture_end();
  fixtureEnded = ended;
  fixtureBoard = [NSPasteboard pasteboardWithUniqueName];
  NSString *kind = @(kindText), *path = @(pathText);
  NSData *data = [NSData dataWithContentsOfFile:path];
  NSPasteboardItem *item = [NSPasteboardItem new];
  if ([kind isEqual:@"paired-file-promise"] || [kind isEqual:@"paired-withdrawn-promise"]) {
    TBLegacyPictures *promise = [TBLegacyPictures new]; promise.data = data; promise.withdrawn = [kind isEqual:@"paired-withdrawn-promise"]; fixtureLegacy = promise; fixturePairedPromise = true;
    [item setString:promise.withdrawn ? [NSURL fileURLWithPath:path].absoluteString : @"file:///missing/promised.png" forType:NSPasteboardTypeFileURL];
  } else if ([kind isEqual:@"legacy"] || [kind isEqual:@"legacy-error"] || [kind isEqual:@"withdrawn"]) {
    TBLegacyPictures *promise = [TBLegacyPictures new]; promise.data = data; promise.failFirst = [kind isEqual:@"legacy-error"]; promise.withdrawn = [kind isEqual:@"withdrawn"]; fixtureLegacy = promise;
    [item setString:[NSURL fileURLWithPath:path].absoluteString forType:NSPasteboardTypeFileURL];
  } else if ([kind isEqual:@"url"] || [kind isEqual:@"url-fixture"] || [kind isEqual:@"link"] || [kind isEqual:@"linked-image"]) {
    [item setString:path forType:NSPasteboardTypeURL];
    if ([kind isEqual:@"url"] || [kind isEqual:@"url-fixture"]) [item setString:[NSString stringWithFormat:@"<img src=\"%@\">", path] forType:NSPasteboardTypeHTML];
    if ([kind isEqual:@"linked-image"]) [item setString:@"<a href=\"https://example.invalid/action\"><img src=\"https://example.invalid/button.png\"></a>" forType:NSPasteboardTypeHTML];
  } else if ([kind isEqual:@"file-preview"]) {
    [item setString:[NSURL fileURLWithPath:path].absoluteString forType:NSPasteboardTypeFileURL];
    [item setData:[NSImage imageNamed:NSImageNameFolder].TIFFRepresentation forType:NSPasteboardTypeTIFF];
  } else if ([kind isEqual:@"protected"]) {
    fixtureProtectedPath = [path stringByAppendingString:@".file.png"];
    [item setString:[NSURL fileURLWithPath:[path stringByAppendingString:@".file.png"]].absoluteString forType:NSPasteboardTypeFileURL];
    [item setData:data forType:NSPasteboardTypePNG];
  } else if ([kind isEqual:@"unreadable"]) {
    [item setString:@"file:///missing/photo.heic" forType:NSPasteboardTypeFileURL];
    [item setData:data forType:NSPasteboardTypePNG];
  } else if ([kind isEqual:@"heic"]) {
    [item setData:data forType:@"public.heic"];
  } else if ([kind isEqual:@"photos"]) {
    [item setString:@"file:///Users/test/Pictures/Photos.photoslibrary/originals/locked.heic" forType:NSPasteboardTypeFileURL];
    [item setData:data forType:NSPasteboardTypePNG];
  } else if ([kind isEqual:@"file"] || [kind isEqual:@"slow-file"] || [kind isEqual:@"slow-file-bytes"]) {
    [item setString:[NSURL fileURLWithPath:path].absoluteString forType:NSPasteboardTypeFileURL];
    if ([kind isEqual:@"slow-file-bytes"]) [item setData:data forType:NSPasteboardTypePNG];
  } else {
    [item setData:data forType:NSPasteboardTypePNG];
    [item setString:@"https://example.invalid/photo.png" forType:NSPasteboardTypeURL];
    if ([kind isEqual:@"rich-text"]) [item setString:@"copied words" forType:NSPasteboardTypeString];
  }
  if ([kind isEqual:@"photos"]) {
    NSPasteboardItem *other = [NSPasteboardItem new];
    [other setString:[NSURL fileURLWithPath:path].absoluteString forType:NSPasteboardTypeFileURL];
    [fixtureBoard writeObjects:@[item, other]];
  } else {
    [fixtureBoard writeObjects:@[item]];
  }
}
// A scripted sender reaches GPUI through the real AppKit methods, including
// the filenames adapter. No pointer or window-server events are posted.
@interface TBComposerFixtureDrag : NSObject
@property(nonatomic, strong) NSWindow *window;
@property(nonatomic) NSPoint location;
@end
@implementation TBComposerFixtureDrag
- (NSPasteboard *)draggingPasteboard { return fixtureBoard; }
- (id)draggingSource { return nil; }
- (NSPoint)draggingLocation { return self.location; }
@end
static TBComposerFixtureDrag *fixtureDrag;
void tb_composer_fixture_drag(void *viewPointer, int phase, const char *vanishingPath) {
  NSView *view = (__bridge NSView *)viewPointer;
  if (phase == 0) {
    fixtureDrag = [TBComposerFixtureDrag new];
    fixtureDrag.window = view.window;
    fixtureDrag.location = NSMakePoint(650, view.window.contentView.bounds.size.height - 300);
  }
  TBComposerFixtureDrag *info = fixtureDrag;
  NSString *vanish = vanishingPath ? @(vanishingPath) : nil;
  // Each callback runs outside App::update. The driver draws a frame between
  // phases, since a covered test window has no display ticks on a Mac.
  dispatch_async(dispatch_get_main_queue(), ^{
    id<NSDraggingDestination> window = (id)info.window;
    if (phase == 0) [window draggingEntered:(id)info];
    else if (phase == 1) {
      info.location = NSMakePoint(info.location.x + 3, info.location.y - 3);
      [window draggingUpdated:(id)info];
    } else {
      [window performDragOperation:(id)info];
      if (vanish) [[NSFileManager defaultManager] removeItemAtPath:vanish error:nil];
      [window concludeDragOperation:(id)info];
      tb_composer_fixture_end();
      fixtureDrag = nil;
    }
  });
}
#endif
