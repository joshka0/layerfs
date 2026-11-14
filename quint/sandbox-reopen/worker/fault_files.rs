use std::{
    fs::File,
    os::unix::io::AsRawFd,
    path::{Path, PathBuf},
    sync::Mutex,
};
static FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
fn flag(path: &Path, enabled: bool) {
    extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
    assert!(
        path.starts_with(std::env::temp_dir()),
        "temporary fault file"
    );
    let file = File::open(path).expect("fault file");
    let mut flags: i64 = 0;
    // SAFETY: Linux copies the inode flags through a live integer pointer.
    assert_eq!(
        unsafe { ioctl(file.as_raw_fd(), 0x80086601u64, &mut flags) },
        0,
        "read immutable flags"
    );
    flags = if enabled { flags | 0x10 } else { flags & !0x10 };
    // SAFETY: the descriptor and flags pointer remain live during ioctl.
    assert_eq!(
        unsafe { ioctl(file.as_raw_fd(), 0x40086602u64, &flags) },
        0,
        "set immutable flags"
    );
}
pub fn refuse(path: PathBuf) {
    flag(&path, true);
    FILES.lock().expect("fault files").push(path);
}
pub fn release() {
    for path in FILES.lock().expect("fault files").drain(..) {
        flag(&path, false);
    }
}
