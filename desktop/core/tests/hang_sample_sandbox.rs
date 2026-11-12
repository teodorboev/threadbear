// The real thing: the hang watch's sandbox profile has to parse, and `sample`
// still has to read a stack from inside it, or the watch would quietly stop
// finding hangs (HangSampleTests' `testSampleStillWorksInTheSandbox`). Out
// here rather than beside hang_sample.rs, as nothing in src/ starts programs
// (open_gate_source.rs).
#[cfg(target_os = "macos")]
#[test]
fn sample_still_works_in_the_sandbox() {
    use std::process::{Command, Stdio};
    use threadbear_core::hang_sample::{heaviest_main_thread_path, sample_arguments};
    let dir = std::env::temp_dir().join(format!("tb-hang-sample-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut target = Command::new("/bin/sleep").arg("20").spawn().unwrap();
    let file = dir.join("s.sample");
    let home = std::env::var("HOME").unwrap_or_default();
    let status = Command::new("/usr/bin/sandbox-exec").args(sample_arguments(&home, target.id(), 1, &file.to_string_lossy())).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap();
    let _ = target.kill();
    let _ = target.wait();
    assert!(status.success());
    let text = std::fs::read_to_string(&file).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!heaviest_main_thread_path(&text).is_empty());
}
