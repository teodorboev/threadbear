// Compile alone: importing the implementation exercises its actual private
// pane/job lifecycle without adding a release test API or opening a window.
#import "../../native/media_views.m"
#import <objc/runtime.h>
#include <assert.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
static int changes;
static BOOL slowCopy;
static dispatch_semaphore_t copyEntered, copyRelease, copyDone;
static NSString *snapshotPath;
typedef void (^Answer)(QLThumbnailRepresentation *,NSError *);
static Answer heldAnswer;
void tb_media_said_link(const char *url,const char *base){(void)url;(void)base;}
void tb_media_said_changed(void){assert(NSThread.isMainThread);changes++;}
bool tb_media_snapshot(int fd,const char *name,char *out,int capacity){(void)fd;(void)name;(void)out;(void)capacity;return false;}
int tb_preview_snapshot(int fd,const char *name,char *out,int capacity,uint64_t limit){
  (void)name;assert(!NSThread.isMainThread);assert(limit==256ULL*1024*1024);
  struct stat info;assert(fstat(fd,&info)==0);
  if(slowCopy){dispatch_semaphore_signal(copyEntered);dispatch_semaphore_wait(copyRelease,DISPATCH_TIME_FOREVER);assert(fstat(fd,&info)==0);}
  char folder[]="/tmp/tb-thumbnail-job-XXXXXX";assert(mkdtemp(folder));
  NSString *file=[[NSString stringWithUTF8String:folder]stringByAppendingPathComponent:@"held.key"];
  assert([@"held document" writeToFile:file atomically:YES encoding:NSUTF8StringEncoding error:nil]);
  assert(strlen(file.fileSystemRepresentation)+1<(size_t)capacity);strcpy(out,file.fileSystemRepresentation);
  snapshotPath=file;if(copyDone)dispatch_semaphore_signal(copyDone);return 1;
}
@interface TBJobGenerator : NSObject
@end
@implementation TBJobGenerator
- (void)generateBestRepresentationForRequest:(QLThumbnailGenerationRequest *)request completionHandler:(Answer)answer{
  (void)request;assert(NSThread.isMainThread);assert(!heldAnswer);heldAnswer=answer;
}
- (void)cancelRequest:(QLThumbnailGenerationRequest *)request{(void)request;}
@end
static id generator(id cls,SEL selector){(void)cls;(void)selector;return[TBJobGenerator new];}
static void runFor(double seconds){[[NSRunLoop currentRunLoop]runUntilDate:[NSDate dateWithTimeIntervalSinceNow:seconds]];}
static void waitFor(BOOL(^ready)(void)){
  NSDate *end=[NSDate dateWithTimeIntervalSinceNow:5];
  while(!ready()){assert(end.timeIntervalSinceNow>0);runFor(.005);}
}
static int openSource(NSString *path){int fd=open(path.fileSystemRepresentation,O_RDONLY);assert(fd>=0);return fd;}
static void answerAndClean(void){
  assert(heldAnswer);Answer answer=heldAnswer;heldAnswer=nil;answer(nil,nil);answer=nil;
  waitFor(^BOOL{return access(snapshotPath.fileSystemRepresentation,F_OK)==-1;});
}
int main(void){setbuf(stdout,NULL);@autoreleasepool{
  [NSApplication sharedApplication];
  NSString *work=[NSTemporaryDirectory()stringByAppendingPathComponent:NSUUID.UUID.UUIDString];
  assert([[NSFileManager defaultManager]createDirectoryAtPath:work withIntermediateDirectories:NO attributes:nil error:nil]);
  NSString *package=[work stringByAppendingPathComponent:@"held.key"];
  assert([[NSFileManager defaultManager]createDirectoryAtPath:package withIntermediateDirectories:NO attributes:nil error:nil]);
  NSString *sentinel=[work stringByAppendingPathComponent:@"keep-me"];
  assert([@"outside snapshot" writeToFile:sentinel atomically:YES encoding:NSUTF8StringEncoding error:nil]);
  Method method=class_getClassMethod(QLThumbnailGenerator.class,@selector(sharedGenerator));IMP previous=method_setImplementation(method,(IMP)generator);
  // Package lengths at the pane boundary are zero. The real directory's stat
  // size still participates in version identity, not a document byte budget.
  int fd=openSource(package);TBQuickLookView *pane;@autoreleasepool{pane=tb_quicklook(fd,0,package);}
  waitFor(^BOOL{return heldAnswer!=nil;});runFor(.15);
  assert(pane.loading && "A pane must not inherit the hover deadline");
  assert(changes==0);answerAndClean();assert(!pane.loading);assert(changes==1);
  assert([pane.failure isEqualToString:@"Quick Look couldn't make a preview of this document."]);
  assert(fcntl(fd,F_GETFD)==-1&&errno==EBADF);pane=nil;
  puts("PASS: package length zero is valid; slow panes keep waiting without the hover deadline");
  // Cancel while the descriptor-only snapshot work is deliberately held.
  changes=0;slowCopy=YES;copyEntered=dispatch_semaphore_create(0);copyRelease=dispatch_semaphore_create(0);copyDone=dispatch_semaphore_create(0);
  fd=openSource(package);@autoreleasepool{pane=tb_quicklook(fd,0,package);}
  waitFor(^BOOL{return dispatch_semaphore_wait(copyEntered,DISPATCH_TIME_NOW)==0;});
  [pane.thumbnail cancel];assert(!pane.loading&&changes==1);assert(fcntl(fd,F_GETFD)!=-1);
  snapshotPath=nil;dispatch_semaphore_signal(copyRelease);
  waitFor(^BOOL{return dispatch_semaphore_wait(copyDone,DISPATCH_TIME_NOW)==0;});
  copyDone=nil;waitFor(^BOOL{return access(snapshotPath.fileSystemRepresentation,F_OK)==-1;});
  assert(!heldAnswer&&changes==1);assert(fcntl(fd,F_GETFD)==-1&&errno==EBADF);pane=nil;slowCopy=NO;
  puts("PASS: cancel during copy keeps the descriptor alive, removes its late private copy and finishes once");
  // A cancelled request may answer later; the private copy must outlive it.
  changes=0;fd=openSource(package);@autoreleasepool{pane=tb_quicklook(fd,0,package);}
  waitFor(^BOOL{return heldAnswer!=nil;});
  TBQuickLookThumbnail *job=pane.thumbnail;[job cancel];[job cancel];job=nil;
  assert(!pane.loading&&changes==1);assert(access(snapshotPath.fileSystemRepresentation,F_OK)==0);
  answerAndClean();assert(changes==1);assert(access(sentinel.fileSystemRepresentation,F_OK)==0);
  assert(fcntl(fd,F_GETFD)==-1&&errno==EBADF);pane=nil;
  puts("PASS: duplicate cancel and late Quick Look answer complete once and remove only the owned snapshot parent");
  method_setImplementation(method,previous);[[NSFileManager defaultManager]removeItemAtPath:work error:nil];
}}
