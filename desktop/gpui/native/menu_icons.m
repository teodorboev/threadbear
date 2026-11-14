// GPUI's MenuItem has no image field. Add the same system symbols as the
// Swift menu after GPUI has rebuilt it, including nested menu items.
#import <AppKit/AppKit.h>

static void tb_menu_images(NSMenu *menu) {
  static NSDictionary<NSString *, NSString *> *symbols;
  static dispatch_once_t once;
  dispatch_once(&once, ^{ symbols = @{
    @"About Threadbear": @"pawprint", @"New Thread": @"square.and.pencil",
    @"Zoom In": @"plus.magnifyingglass", @"Zoom Out": @"minus.magnifyingglass",
    @"Actual Size": @"1.magnifyingglass", @"Show Threads": @"sidebar.left",
    @"Hide Threads": @"sidebar.left", @"Collapse Threads": @"sidebar.squares.left",
    @"Expand Threads": @"sidebar.squares.left", @"Show Panels": @"sidebar.right",
    @"Hide Panels": @"sidebar.right", @"Show Terminal": @"apple.terminal",
    @"Hide Terminal": @"apple.terminal", @"Reset Panel Layout": @"rectangle.3.group",
    @"Back": @"chevron.left", @"Forward": @"chevron.right",
    @"Next Inbox Thread": @"chevron.down", @"Show Previous Tab": @"chevron.backward.square",
    @"Show Next Tab": @"chevron.forward.square", @"Take the Tour": @"map",
    @"Finish Setting Up": @"checklist", @"Archive": @"archivebox",
    @"Unarchive": @"tray.and.arrow.up", @"Hibernate": @"moon.zzz",
    @"Stop": @"stop.circle", @"Copy Transcript as Markdown": @"doc.plaintext",
    @"Copy Thread ID": @"number", @"Copy thread id": @"number",
    @"Open on Web": @"globe", @"Open on the Web": @"globe",
    @"Share…": @"square.and.arrow.up", @"Pick a time…": @"calendar",
    @"Open Anything…": @"magnifyingglass", @"Settings…": @"gearshape",
    @"Quit Threadbear": @"xmark.square", @"Show Inbox": @"tray",
    @"Show Scheduled": @"clock", @"Show Projects": @"square.stack",
    @"Show Librarian": @"books.vertical", @"Show Hibernating": @"moon.zzz",
    @"Show Archived": @"archivebox", @"1 hour": @"clock",
    @"This evening, 6 pm": @"moon", @"Tomorrow, 9 am": @"sunrise",
    @"Next Monday, 9 am": @"calendar"
  }; });
  for (NSMenuItem *item in menu.itemArray) {
    NSString *symbol = symbols[item.title];
    if ([item.title hasPrefix:@"Restart to Update"]) symbol = @"arrow.clockwise";
    if (symbol) {
      NSImage *image = [NSImage imageWithSystemSymbolName:symbol accessibilityDescription:nil];
      image.size = NSMakeSize(16, 16);
      item.image = image;
    }
    if (item.submenu) tb_menu_images(item.submenu);
  }
}

void tb_menu_icons(void) {
  tb_menu_images(NSApp.mainMenu);
}
