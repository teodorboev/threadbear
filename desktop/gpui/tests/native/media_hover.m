#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <objc/runtime.h>
#import <QuickLookThumbnailing/QuickLookThumbnailing.h>
#include <assert.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>
#include <errno.h>

typedef struct { uint64_t device,inode,length; int64_t modified,modified_ns,changed,changed_ns; } TBMediaVersion;
_Static_assert(sizeof(TBMediaVersion)==56,"matches repr(C) MediaVersion");
extern size_t tb_media_thumbnail(int fd,long long length,const char *name,bool pdf,const TBMediaVersion *expected,unsigned char *out,size_t capacity);
void tb_media_said_link(const char *url,const char *base){(void)url;(void)base;}
void tb_media_said_changed(void){}

// Test stand-in for Rust's private snapshot helper. It copies only the held
// descriptor to a fresh private folder; name and the original path aren't read.
static bool snapshot(int fd,const char *name,char *out,int capacity){
  (void)name;
  char folder[]="/tmp/tb-hover-native-XXXXXX";
  if(!mkdtemp(folder))return false;
  char path[4096];snprintf(path,sizeof path,"%s/hover.pdf",folder);
  int copy=open(path,O_WRONLY|O_CREAT|O_EXCL,0600);
  bool good=copy>=0;
  if(good){
    unsigned char bytes[65536];off_t offset=0;
    for(;;){ssize_t n=pread(fd,bytes,sizeof bytes,offset);if(n<0){good=false;break;}if(!n)break;
      size_t at=0;while(at<(size_t)n){ssize_t wrote=write(copy,bytes+at,n-at);if(wrote<=0){good=false;break;}at+=wrote;}
      if(!good)break;offset+=n;
    }
    close(copy);
  }
  if(!good||strlen(path)+1>(size_t)capacity){unlink(path);rmdir(folder);return false;}
  memcpy(out,path,strlen(path)+1);return true;
}
// The production helper has the same descriptor-only route and a 16 MiB
// cap, including on APFS. Tests below also call Rust's real entrypoint.
bool tb_media_snapshot(int fd,const char *name,char *out,int capacity){return snapshot(fd,name,out,capacity);}
int tb_preview_snapshot(int fd,const char *name,char *out,int capacity,uint64_t limit){
  struct stat info;if(fstat(fd,&info)!=0||!S_ISREG(info.st_mode))return 0;
  if(limit==0||limit>256*1024*1024||(uint64_t)info.st_size>limit)return -1;
  return snapshot(fd,name,out,capacity)?1:0;
}
bool tb_hover_snapshot(int fd,const char *name,char *out,int capacity){
  struct stat info;if(fstat(fd,&info)!=0||!S_ISREG(info.st_mode)||info.st_size>16*1024*1024)return false;
  return snapshot(fd,name,out,capacity);
}
static TBMediaVersion version(int fd){
  struct stat s;assert(fstat(fd,&s)==0);
  return (TBMediaVersion){s.st_dev,s.st_ino,s.st_size,s.st_mtimespec.tv_sec,s.st_mtimespec.tv_nsec,s.st_ctimespec.tv_sec,s.st_ctimespec.tv_nsec};
}
static void pump(dispatch_semaphore_t done){
  NSDate *end=[NSDate dateWithTimeIntervalSinceNow:15];
  while(dispatch_semaphore_wait(done,DISPATCH_TIME_NOW)!=0){
    assert(end.timeIntervalSinceNow>0);
    [[NSRunLoop currentRunLoop]runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:.01]];
  }
}
static size_t render(int fd,long long size,const char *name,bool pdf,TBMediaVersion expected,NSMutableData *out,size_t capacity){
  __block size_t count=0;
  dispatch_semaphore_t done=dispatch_semaphore_create(0);
  dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0),^{
    count=tb_media_thumbnail(fd,size,name,pdf,&expected,out.mutableBytes,capacity);
    dispatch_semaphore_signal(done);
  });
  pump(done);return count;
}
static void check(int fd,long long size,const char *name,bool pdf,bool good,size_t capacity,bool mismatch){
  TBMediaVersion expected=version(fd);if(mismatch)expected.changed_ns^=1;
  NSMutableData *out=[NSMutableData dataWithLength:2*1024*1024];
  size_t n=render(fd,size,name,pdf,expected,out,capacity);
  assert((n>0)==good);
  if(n){
    NSBitmapImageRep *rep=[NSBitmapImageRep imageRepWithData:[NSData dataWithBytes:out.bytes length:n]];
    assert(rep&&rep.pixelsWide>0&&rep.pixelsHigh>0&&rep.pixelsWide<=552&&rep.pixelsHigh<=340);
  }
  assert(fcntl(fd,F_GETFD)==-1&&errno==EBADF);
  // Let the queued Quick Look cancellation release its private snapshot.
  [[NSRunLoop currentRunLoop]runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.1]];
}

#ifdef TB_LATE_CALLBACK_TEST
typedef void (^FrameAnswer)(CMTime,CGImageRef,CMTime,AVAssetImageGeneratorResult,NSError *);
@interface TBDelayedFrame : NSObject
@property BOOL appliesPreferredTrackTransform;
@property CGSize maximumSize;
@property(nonatomic,strong) AVAsset *asset;
@property(nonatomic,copy) FrameAnswer answer;
@end
static __weak TBDelayedFrame *weakGenerator;
static dispatch_semaphore_t callbackDone, callbackRelease;
@implementation TBDelayedFrame
- (void)generateCGImagesAsynchronouslyForTimes:(NSArray *)times completionHandler:(FrameAnswer)answer{
  (void)times;self.answer=answer;
  __weak TBDelayedFrame *weak=self;
  dispatch_async(dispatch_get_global_queue(QOS_CLASS_DEFAULT,0),^{
    dispatch_semaphore_wait(callbackRelease, DISPATCH_TIME_FOREVER);
    TBDelayedFrame *me=weak;assert(me); // catches generator dropped after timeout
    FrameAnswer reply=me.answer;me.answer=nil;
    reply(kCMTimeZero,NULL,kCMTimeZero,AVAssetImageGeneratorCancelled,nil);
    dispatch_semaphore_signal(callbackDone);
  });
}
- (void)cancelAllCGImageGeneration{}
@end
static id delayed(id cls,SEL selector,AVAsset *asset){
  (void)cls;(void)selector;
  TBDelayedFrame *frame=[TBDelayedFrame new];frame.asset=asset;weakGenerator=frame;return frame;
}
#endif

#ifdef TB_LATE_QUICKLOOK_TEST
typedef void (^ThumbnailAnswer)(QLThumbnailRepresentation *,NSError *);
static dispatch_semaphore_t thumbnailRelease, thumbnailDone;
static NSString *heldSnapshot;
static IMP previousThumbnailInit;
static id captureThumbnailInit(id me,SEL selector,NSURL *url,CGSize size,CGFloat scale,QLThumbnailGenerationRequestRepresentationTypes types){
  heldSnapshot=url.path;
  return ((id(*)(id,SEL,NSURL*,CGSize,CGFloat,QLThumbnailGenerationRequestRepresentationTypes))previousThumbnailInit)(me,selector,url,size,scale,types);
}
@interface TBDelayedThumbnail : NSObject
@end
@implementation TBDelayedThumbnail
- (void)generateBestRepresentationForRequest:(QLThumbnailGenerationRequest *)request completionHandler:(ThumbnailAnswer)answer {
  (void)request;NSString *path=heldSnapshot;
  dispatch_async(dispatch_get_global_queue(QOS_CLASS_DEFAULT,0),^{
    dispatch_semaphore_wait(thumbnailRelease,DISPATCH_TIME_FOREVER);
    assert(access(path.fileSystemRepresentation,F_OK)==0);
    answer(nil,nil);
    dispatch_semaphore_signal(thumbnailDone);
  });
}
- (void)cancelRequest:(QLThumbnailGenerationRequest *)request{(void)request;}
@end
static id delayedThumbnail(id cls,SEL selector){(void)cls;(void)selector;return[TBDelayedThumbnail new];}
#endif

// A few solid green frames, made by the system encoder: no checked-in
// media, ffmpeg dependency, screen recording, network or private fixtures.
static NSString *film(void){
  NSString *path=[NSTemporaryDirectory()stringByAppendingPathComponent:[NSUUID.UUID.UUIDString stringByAppendingString:@".mp4"]];
  NSError *error=nil;
  AVAssetWriter *writer=[AVAssetWriter assetWriterWithURL:[NSURL fileURLWithPath:path]fileType:AVFileTypeMPEG4 error:&error];assert(writer&&!error);
  AVAssetWriterInput *input=[AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeVideo outputSettings:@{AVVideoCodecKey:AVVideoCodecTypeH264,AVVideoWidthKey:@64,AVVideoHeightKey:@48}];
  AVAssetWriterInputPixelBufferAdaptor *adaptor=[AVAssetWriterInputPixelBufferAdaptor assetWriterInputPixelBufferAdaptorWithAssetWriterInput:input sourcePixelBufferAttributes:@{(id)kCVPixelBufferPixelFormatTypeKey:@(kCVPixelFormatType_32BGRA),(id)kCVPixelBufferWidthKey:@64,(id)kCVPixelBufferHeightKey:@48}];
  [writer addInput:input];assert([writer startWriting]);[writer startSessionAtSourceTime:kCMTimeZero];
  CVPixelBufferRef pixels=NULL;assert(CVPixelBufferCreate(kCFAllocatorDefault,64,48,kCVPixelFormatType_32BGRA,NULL,&pixels)==kCVReturnSuccess);
  CVPixelBufferLockBaseAddress(pixels,0);unsigned char *base=CVPixelBufferGetBaseAddress(pixels);size_t stride=CVPixelBufferGetBytesPerRow(pixels);
  for(int y=0;y<48;y++)for(int x=0;x<64;x++){unsigned char *p=base+y*stride+x*4;p[0]=0;p[1]=190;p[2]=0;p[3]=255;}
  CVPixelBufferUnlockBaseAddress(pixels,0);
  NSDate *end=[NSDate dateWithTimeIntervalSinceNow:10];
  for(int n=0;n<3;n++){
    while(!input.readyForMoreMediaData){assert(end.timeIntervalSinceNow>0);[[NSRunLoop currentRunLoop]runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.01]];}
    assert([adaptor appendPixelBuffer:pixels withPresentationTime:CMTimeMake(n,10)]);
  }
  CVPixelBufferRelease(pixels);[input markAsFinished];dispatch_semaphore_t done=dispatch_semaphore_create(0);
  [writer finishWritingWithCompletionHandler:^{dispatch_semaphore_signal(done);}];pump(done);assert(writer.status==AVAssetWriterStatusCompleted);
  return path;
}

int main(int argc,char **argv){@autoreleasepool{
  [NSApplication sharedApplication];
  NSString *made=argc>1?nil:film();
  const char *movie=argc>1?argv[1]:made.fileSystemRepresentation;
#ifdef TB_LATE_CALLBACK_TEST
  int fd=open(movie,O_RDONLY);assert(fd>=0);
  struct stat st;assert(fstat(fd,&st)==0);
  callbackDone=dispatch_semaphore_create(0);callbackRelease=dispatch_semaphore_create(0);
  Method method=class_getClassMethod(AVAssetImageGenerator.class,@selector(assetImageGeneratorWithAsset:));
  IMP previous=method_setImplementation(method,(IMP)delayed);
  NSMutableData *out=[NSMutableData dataWithLength:2*1024*1024];
  assert(render(fd,st.st_size,"/missing/held.mp4",false,version(fd),out,out.length)==0);
  assert(fcntl(fd,F_GETFD)!=-1); // descriptor must outlive the late callback
  dispatch_semaphore_signal(callbackRelease);
  pump(callbackDone);
  for(int n=0;n<100&&weakGenerator;n++){[[NSRunLoop currentRunLoop]runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.01]];}
  assert(!weakGenerator);assert(fcntl(fd,F_GETFD)==-1&&errno==EBADF);
  method_setImplementation(method,previous);
  puts("PASS: timeout keeps video decoder and descriptor alive until its late callback");
#elif defined(TB_LATE_QUICKLOOK_TEST)
  NSString *file=[NSTemporaryDirectory()stringByAppendingPathComponent:[NSUUID.UUID.UUIDString stringByAppendingString:@".pdf"]];
  NSMutableData *data=[NSMutableData data];
  CGDataConsumerRef consumer=CGDataConsumerCreateWithCFData((__bridge CFMutableDataRef)data);
  CGRect rect=CGRectMake(0,0,300,500);CGContextRef context=CGPDFContextCreate(consumer,&rect,NULL);
  CGPDFContextBeginPage(context,NULL);CGContextSetRGBFillColor(context,1,0,0,1);CGContextFillRect(context,CGRectMake(30,30,120,250));
  CGPDFContextEndPage(context);CGPDFContextClose(context);CGContextRelease(context);CGDataConsumerRelease(consumer);
  assert([data writeToFile:file atomically:YES]);
  int fd=open(file.fileSystemRepresentation,O_RDONLY);assert(fd>=0);
  thumbnailDone=dispatch_semaphore_create(0);thumbnailRelease=dispatch_semaphore_create(0);
  Method method=class_getClassMethod(QLThumbnailGenerator.class,@selector(sharedGenerator));
  IMP previous=method_setImplementation(method,(IMP)delayedThumbnail);
  Method init=class_getInstanceMethod(QLThumbnailGenerationRequest.class,@selector(initWithFileAtURL:size:scale:representationTypes:));
  previousThumbnailInit=method_setImplementation(init,(IMP)captureThumbnailInit);
  NSMutableData *out=[NSMutableData dataWithLength:2*1024*1024];
  assert(render(fd,data.length,file.fileSystemRepresentation,true,version(fd),out,out.length)==0);
  assert(heldSnapshot&&access(heldSnapshot.fileSystemRepresentation,F_OK)==0);
  assert(fcntl(fd,F_GETFD)==-1&&errno==EBADF);
  dispatch_semaphore_signal(thumbnailRelease);pump(thumbnailDone);
  for(int n=0;n<100&&access(heldSnapshot.fileSystemRepresentation,F_OK)==0;n++)[[NSRunLoop currentRunLoop]runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.01]];
  assert(access(heldSnapshot.fileSystemRepresentation,F_OK)==-1);
  method_setImplementation(init,previousThumbnailInit);method_setImplementation(method,previous);unlink(file.fileSystemRepresentation);
  puts("PASS: cancelled Quick Look keeps its private snapshot through a late callback, then removes it");
#else
  NSString *file=[NSTemporaryDirectory()stringByAppendingPathComponent:[NSUUID.UUID.UUIDString stringByAppendingString:@".pdf"]];
  NSMutableData *data=[NSMutableData data];
  CGDataConsumerRef consumer=CGDataConsumerCreateWithCFData((__bridge CFMutableDataRef)data);
  CGRect rect=CGRectMake(0,0,300,500);CGContextRef context=CGPDFContextCreate(consumer,&rect,NULL);
  CGPDFContextBeginPage(context,NULL);CGContextSetRGBFillColor(context,1,0,0,1);CGContextFillRect(context,CGRectMake(30,30,120,250));
  CGPDFContextEndPage(context);CGPDFContextClose(context);CGContextRelease(context);CGDataConsumerRelease(consumer);
  assert([data writeToFile:file atomically:YES]);
  int fd=open(file.fileSystemRepresentation,O_RDONLY);assert(fd>=0);
  unlink(file.fileSystemRepresentation);assert(symlink("/missing/thumbnail-source",file.fileSystemRepresentation)==0);
  check(fd,data.length,file.fileSystemRepresentation,true,true,2*1024*1024,false);
  unlink(file.fileSystemRepresentation);assert([data writeToFile:file atomically:YES]);
  check(open(file.fileSystemRepresentation,O_RDONLY),data.length,file.fileSystemRepresentation,true,false,1,false);
  check(open(file.fileSystemRepresentation,O_RDONLY),data.length,file.fileSystemRepresentation,true,false,2*1024*1024,true);
  check(open(file.fileSystemRepresentation,O_RDONLY),17*1024*1024,file.fileSystemRepresentation,true,false,2*1024*1024,false);
  unlink(file.fileSystemRepresentation);
  {struct stat st;assert(stat(movie,&st)==0);check(open(movie,O_RDONLY),st.st_size,"/missing/replaced.mp4",false,true,2*1024*1024,false);}
  puts("PASS: held-descriptor PDF/video raster, bounds, output cap, version mismatch and descriptor closure");
#endif
  if(made)unlink(made.fileSystemRepresentation);
} }
