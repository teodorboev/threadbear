// threadbear-web-helper: the processes Chromium starts beside the app (its
// renderers, GPU process and so on). Built with the app so the two always
// match, shipped beside it, and copied into the web runtime's folder as the
// "Threadbear Helper" apps (threadbear_web::helper_main says the rest).
fn main() {
    std::process::exit(threadbear_web::helper_main());
}
