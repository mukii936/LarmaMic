//! Chút việc riêng của Windows: ẩn/hiện cửa sổ (chạy nền ở khay) và khởi động cùng Windows.

use crate::i18n::{t, tf};
use std::os::windows::process::CommandExt;
use std::process::Command;

#[link(name = "user32")]
extern "system" {
    fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    fn SetForegroundWindow(hwnd: isize) -> i32;
    fn IsWindowVisible(hwnd: isize) -> i32;
    fn IsIconic(hwnd: isize) -> i32;
    fn IsWindow(hwnd: isize) -> i32;
    fn EnumWindows(cb: unsafe extern "system" fn(isize, isize) -> i32, lparam: isize) -> i32;
    fn GetWindowTextW(hwnd: isize, buf: *mut u16, max: i32) -> i32;
    fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn Beep(freq: u32, ms: u32) -> i32;
    fn GetCurrentProcess() -> isize;
    fn K32EmptyWorkingSet(process: isize) -> i32;
}

// ---- chặn nút đóng cửa sổ ở tầng Windows ----
// Nếu để egui xử lý (CancelClose), nó tự hẹn vẽ lại một khung; cửa sổ đã ẩn thì không bao giờ vẽ được
// khung đó nên vòng lặp sự kiện quay không nghỉ, ngốn trọn một nhân CPU. Vì vậy ta bắt WM_CLOSE trước,
// chỉ báo cho giao diện "hãy ẩn đi", và giao diện ẩn cửa sổ trong một khung không hẹn vẽ gì thêm.
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering::SeqCst};
use std::sync::OnceLock;

static OLD_PROC: AtomicIsize = AtomicIsize::new(0);
static HIDE_REQ: AtomicBool = AtomicBool::new(false);
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

#[link(name = "user32")]
extern "system" {
    fn SetWindowLongPtrW(hwnd: isize, index: i32, new: isize) -> isize;
    fn CallWindowProcW(prev: isize, hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
}

// ---- đổi tooltip của icon khay từ luồng khác ----
// TrayIcon không chuyển luồng được, nên luồng sự kiện gửi thông điệp tới cửa sổ chính; hàm cửa sổ chạy
// trên luồng chính (nơi giữ icon) và tự đổi tooltip, kể cả khi cửa sổ đang ẩn.
use std::cell::RefCell;
use std::sync::Mutex;

const WM_TIP: u32 = 0x8001; // WM_APP + 1
static TIP: Mutex<String> = Mutex::new(String::new());

thread_local! {
    static TIP_HANDLER: RefCell<Option<Box<dyn Fn(&str)>>> = const { RefCell::new(None) };
}

/// Gọi trên luồng chính: đăng ký hàm đổi tooltip (giữ icon khay bên trong).
pub fn set_tip_handler(f: impl Fn(&str) + 'static) {
    TIP_HANDLER.with(|c| *c.borrow_mut() = Some(Box::new(f)));
}

const WM_RELABEL: u32 = 0x8002; // WM_APP + 2

thread_local! {
    static RELABEL_HANDLER: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

/// Gọi trên luồng chính: đăng ký hàm đổi chữ menu khay (giữ các mục menu bên trong).
pub fn set_relabel_handler(f: impl Fn() + 'static) {
    RELABEL_HANDLER.with(|c| *c.borrow_mut() = Some(Box::new(f)));
}

/// Gọi từ luồng bất kỳ: nhờ luồng chính đặt lại chữ menu khay theo ngôn ngữ hiện tại.
pub fn relabel_tray() {
    if OLD_PROC.load(SeqCst) == 0 {
        return;
    }
    let h = find();
    if h != 0 {
        unsafe { PostMessageW(h, WM_RELABEL, 0, 0) };
    }
}

/// Gọi từ luồng bất kỳ: nhờ luồng chính đổi tooltip. Trả về false nếu chưa gửi được (thử lại sau).
pub fn request_tip(text: &str) -> bool {
    if OLD_PROC.load(SeqCst) == 0 {
        return false; // hàm cửa sổ chưa được cài
    }
    if let Ok(mut t) = TIP.lock() {
        *t = text.to_string();
    }
    let h = find();
    h != 0 && unsafe { PostMessageW(h, WM_TIP, 0, 0) != 0 }
}

unsafe extern "system" fn close_proc(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize {
    const WM_CLOSE: u32 = 0x0010;
    if msg == WM_TIP {
        let text = TIP.lock().map(|t| t.clone()).unwrap_or_default();
        TIP_HANDLER.with(|c| {
            if let Some(f) = c.borrow().as_ref() {
                f(&text);
            }
        });
        return 0;
    }
    if msg == WM_RELABEL {
        RELABEL_HANDLER.with(|c| {
            if let Some(f) = c.borrow().as_ref() {
                f();
            }
        });
        return 0;
    }
    if msg == WM_CLOSE {
        HIDE_REQ.store(true, SeqCst);
        if let Some(wake) = WAKE.get() {
            wake(); // đánh thức giao diện để nó ẩn cửa sổ
        }
        return 0; // nuốt sự kiện: cửa sổ không bị đóng
    }
    CallWindowProcW(OLD_PROC.load(SeqCst), hwnd, msg, wparam, lparam)
}

/// Gọi một lần, từ luồng giao diện, sau khi cửa sổ đã tạo. `wake` đánh thức vòng lặp giao diện.
pub fn install_close_hook(wake: impl Fn() + Send + Sync + 'static) -> bool {
    if OLD_PROC.load(SeqCst) != 0 {
        return true;
    }
    let h = find();
    if h == 0 {
        return false;
    }
    let _ = WAKE.set(Box::new(wake));
    let proc_ptr: unsafe extern "system" fn(isize, u32, usize, isize) -> isize = close_proc;
    let old = unsafe { SetWindowLongPtrW(h, -4, proc_ptr as usize as isize) }; // GWLP_WNDPROC
    OLD_PROC.store(old, SeqCst);
    old != 0
}

/// Người dùng vừa bấm nút đóng (đã bị chặn): giao diện cần ẩn cửa sổ?
pub fn take_hide_request() -> bool {
    HIDE_REQ.swap(false, SeqCst)
}

/// Cửa sổ chính đang hiện và không bị thu nhỏ? Nếu không thì giao diện không cần vẽ lại.
pub fn is_visible() -> bool {
    let h = find();
    h != 0 && unsafe { IsWindowVisible(h) != 0 && IsIconic(h) == 0 }
}

/// Trả bộ nhớ không dùng về cho Windows (khi app thu vào khay, RAM hiện trong Task Manager giảm hẳn).
pub fn trim_memory() {
    unsafe {
        K32EmptyWorkingSet(GetCurrentProcess());
    }
}

#[repr(C)]
struct SystemTime {
    year: u16,
    month: u16,
    dow: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    ms: u16,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLocalTime(t: *mut SystemTime);
}

/// Giờ địa phương: (năm, tháng, ngày, giờ, phút, giây).
pub fn local_time() -> (u16, u16, u16, u16, u16, u16) {
    let mut t = SystemTime { year: 0, month: 0, dow: 0, day: 0, hour: 0, minute: 0, second: 0, ms: 0 };
    unsafe { GetLocalTime(&mut t) };
    (t.year, t.month, t.day, t.hour, t.minute, t.second)
}

pub const TITLE: &str = "LarmaMic";

/// Tiếng bíp báo trạng thái: thấp = vừa tắt tiếng, cao = vừa bật tiếng.
pub fn beep(muted: bool) {
    unsafe {
        if muted {
            Beep(420, 150);
        } else {
            Beep(950, 90);
        }
    }
}

struct Search {
    pid: u32, // 0 = cửa sổ của bất kỳ tiến trình nào
    found: isize,
}

unsafe extern "system" fn enum_cb(hwnd: isize, lparam: isize) -> i32 {
    let s = &mut *(lparam as *mut Search);
    let mut buf = [0u16; 32];
    let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), 32);
    if n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == TITLE {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if s.pid == 0 || pid == s.pid {
            s.found = hwnd;
            return 0; // dừng duyệt
        }
    }
    1
}

fn search(pid: u32) -> isize {
    let mut s = Search { pid, found: 0 };
    unsafe { EnumWindows(enum_cb, &mut s as *mut Search as isize) };
    s.found
}

static OWN: AtomicIsize = AtomicIsize::new(0);

/// Cửa sổ chính CỦA TIẾN TRÌNH NÀY. Tìm theo tiêu đề thôi thì dễ nhầm sang cửa sổ cùng tên của bản khác
/// (ví dụ bản thử chạy cạnh bản đang dùng), làm ẩn/hiện/vẽ lại sai cửa sổ. Kết quả được nhớ lại.
fn find() -> isize {
    let c = OWN.load(SeqCst);
    if c != 0 && unsafe { IsWindow(c) } != 0 {
        return c;
    }
    let h = search(std::process::id());
    OWN.store(h, SeqCst);
    h
}

fn show(h: isize) {
    if h != 0 {
        unsafe {
            ShowWindow(h, 9); // SW_RESTORE
            ShowWindow(h, 5); // SW_SHOW
            SetForegroundWindow(h);
        }
    }
}

/// Hiện lại cửa sổ chính của chính tiến trình này (kể cả khi đang ẩn trong khay).
pub fn show_main() {
    show(find());
}

/// Mở app lần hai: gọi cửa sổ của bản đang chạy lên.
pub fn show_other() {
    show(search(0));
}

/// Ẩn cửa sổ chính: app vẫn chạy, chỉ còn icon ở khay hệ thống.
pub fn hide_main() {
    let h = find();
    if h != 0 {
        unsafe {
            ShowWindow(h, 0); // SW_HIDE
        }
        // Ẩn cửa sổ làm Windows/winit phát sinh vài sự kiện khiến eframe xin vẽ lại một khung, mà cửa sổ ẩn
        // thì không bao giờ vẽ nên vòng lặp quay không nghỉ. Gửi WM_PAINT sau đó để "xả" các yêu cầu treo.
        std::thread::spawn(move || {
            for ms in [150u64, 600] {
                std::thread::sleep(std::time::Duration::from_millis(ms));
                unsafe {
                    PostMessageW(h, 0x000F, 0, 0); // WM_PAINT
                }
            }
            trim_memory();
        });
        trim_memory();
    }
}

// ---- khởi động cùng Windows (khóa Run của người dùng hiện tại) ----
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "LarmaMic";

fn reg(args: &[&str]) -> bool {
    Command::new("reg")
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Giá trị trong khóa Run có tên `value` đang tồn tại?
pub fn run_entry_exists(value: &str) -> bool {
    reg(&["query", RUN_KEY, "/v", value])
}

/// Bật/tắt một giá trị trong khóa Run, trỏ tới `exe` (kèm `--background` để khởi động ẩn ở khay).
pub fn set_run_entry(value: &str, exe: &str, on: bool) -> bool {
    if on {
        let cmd = format!("\"{exe}\" --background");
        reg(&["add", RUN_KEY, "/v", value, "/t", "REG_SZ", "/d", &cmd, "/f"])
    } else {
        reg(&["delete", RUN_KEY, "/v", value, "/f"])
    }
}

pub fn autostart_enabled() -> bool {
    run_entry_exists(VALUE)
}

pub fn set_autostart(on: bool) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    set_run_entry(VALUE, &exe.display().to_string(), on)
}

// ---- cài đặt / gỡ cài đặt (không cần trình cài ngoài) ----
// Cài: chép exe vào %LOCALAPPDATA%\Programs\LarmaMic, tạo lối tắt Start Menu và mục "Gỡ cài đặt" trong
// Settings > Apps. Gỡ: `larma-mic.exe --uninstall`. Chế độ `--test` dùng tên khác để thử mà không đụng
// bản cài thật.
use std::path::PathBuf;

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, flags: u32) -> i32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Hộp thoại thông báo. `yes_no` = hỏi Có/Không (mặc định Không); trả về true nếu bấm Có (hoặc OK).
pub fn message_box(title: &str, text: &str, yes_no: bool) -> bool {
    let flags = if yes_no { 0x4 | 0x20 | 0x100 } else { 0x40 }; // YESNO | ICONQUESTION | DEFBUTTON2 | ICONINFO
    let r = unsafe { MessageBoxW(0, wide(text).as_ptr(), wide(title).as_ptr(), flags) };
    !yes_no || r == 6
}

pub struct InstallLayout {
    pub name: &'static str,
    pub dir: PathBuf,
    pub exe: PathBuf,
    pub lnk: PathBuf,
    pub key: String,
}

pub fn install_layout(test: bool) -> InstallLayout {
    let name = if test { "LarmaMic-Test" } else { "LarmaMic" };
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = local.join("Programs").join(name);
    InstallLayout {
        name,
        exe: dir.join("larma-mic.exe"),
        lnk: roaming.join(r"Microsoft\Windows\Start Menu\Programs").join(format!("{name}.lnk")),
        key: format!(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{name}"),
        dir,
    }
}

/// Exe đang chạy chính là bản đã cài?
pub fn is_installed() -> bool {
    let l = install_layout(false);
    std::env::current_exe()
        .map(|e| e.to_string_lossy().eq_ignore_ascii_case(&l.exe.to_string_lossy()))
        .unwrap_or(false)
}

/// Ghi một dòng vào %APPDATA%\LarmaMic\app.log (cài đặt/gỡ, lý do ngắt kết nối...). Giữ tối đa ~200 KB.
pub fn log_line(msg: &str) {
    let (y, mo, d, h, mi, s) = local_time();
    let dir = crate::settings::dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("app.log");
    if std::fs::metadata(&path).map(|m| m.len() > 200_000).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("app.log.old"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = writeln!(f, "{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} {msg}");
    }
}

fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

pub fn install(test: bool) -> Result<String, String> {
    let l = install_layout(test);
    let cur = std::env::current_exe().map_err(|e| tf("inst.err.exe", &[&e.to_string()]))?;
    if cur.to_string_lossy().eq_ignore_ascii_case(&l.exe.to_string_lossy()) {
        return Ok(t("inst.already"));
    }
    std::fs::create_dir_all(&l.dir).map_err(|e| tf("inst.err.dir", &[&e.to_string()]))?;
    std::fs::copy(&cur, &l.exe)
        .map_err(|e| tf("inst.err.copy", &[&l.exe.display().to_string(), &e.to_string()]))?;

    // lối tắt Start Menu
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{lnk}');$s.TargetPath='{exe}';\
         $s.WorkingDirectory='{dir}';$s.IconLocation='{exe},0';$s.Description='{desc}';$s.Save()",
        desc = ps_quote(&t("app.tagline")),
        lnk = ps_quote(&l.lnk.display().to_string()),
        exe = ps_quote(&l.exe.display().to_string()),
        dir = ps_quote(&l.dir.display().to_string()),
    );
    let ok = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        log_line(&t("inst.warn.lnk"));
    }

    // mục "Gỡ cài đặt" trong Settings > Apps
    let exe_s = l.exe.display().to_string();
    let unin = format!("\"{exe_s}\" --uninstall{}", if test { " --test" } else { "" });
    let size_kb = std::fs::metadata(&l.exe).map(|m| (m.len() / 1024).to_string()).unwrap_or_else(|_| "9000".into());
    let ver = env!("CARGO_PKG_VERSION");
    for (name, ty, val) in [
        ("DisplayName", "REG_SZ", l.name),
        ("DisplayVersion", "REG_SZ", ver),
        ("Publisher", "REG_SZ", "LarmaMic"),
        ("DisplayIcon", "REG_SZ", exe_s.as_str()),
        ("InstallLocation", "REG_SZ", &l.dir.display().to_string()),
        ("UninstallString", "REG_SZ", unin.as_str()),
        ("EstimatedSize", "REG_DWORD", size_kb.as_str()),
        ("NoModify", "REG_DWORD", "1"),
        ("NoRepair", "REG_DWORD", "1"),
    ] {
        if !reg(&["add", &l.key, "/v", name, "/t", ty, "/d", val, "/f"]) {
            return Err(tf("inst.err.reg", &[name]));
        }
    }

    // đang bật "chạy cùng Windows" thì trỏ sang bản đã cài
    if !test && autostart_enabled() {
        set_run_entry(VALUE, &exe_s, true);
    }
    let msg = tf("inst.installed", &[&l.dir.display().to_string()]);
    log_line(&msg);
    Ok(msg)
}

pub fn uninstall(test: bool, quiet: bool) -> Result<String, String> {
    let l = install_layout(test);
    if !quiet
        && !message_box(&t("un.title"), &t("un.confirm"), true)
    {
        return Ok(t("un.cancelled"));
    }
    // đóng bản đang chạy (trừ chính tiến trình gỡ này); chế độ thử thì không đụng bản thật
    if !test {
        let pid = std::process::id().to_string();
        let filter = format!("PID ne {pid}");
        let _ = Command::new("taskkill")
            .args(["/F", "/FI", &filter, "/IM", "larma-mic.exe"])
            .creation_flags(0x0800_0000)
            .output();
        let _ = set_run_entry(VALUE, "", false);
    }
    let _ = std::fs::remove_file(&l.lnk);
    reg(&["delete", &l.key, "/f"]);

    let mut extra = String::new();
    if !test
        && !quiet
        && message_box(&t("un.title"), &t("un.ask.settings"), true)
    {
        let _ = std::fs::remove_dir_all(crate::settings::dir());
        extra = t("un.deleted.settings");
    }

    // exe đang chạy nên không tự xóa được: nhờ một tiến trình phụ xóa thư mục sau khi ta thoát
    // `raw_arg` giữ nguyên dấu ngoặc kép; `/s` bảo cmd bỏ cặp ngoặc kép ngoài cùng.
    let cmd = format!("/s /c \"ping 127.0.0.1 -n 3 >nul & rmdir /s /q \"{}\"\"", l.dir.display());
    let _ = Command::new("cmd")
        .raw_arg(cmd)
        .creation_flags(0x0800_0000 | 0x0000_0008) // CREATE_NO_WINDOW | DETACHED_PROCESS
        .spawn();
    let msg = tf("un.done", &[&extra]);
    log_line(&msg);
    Ok(msg)
}

/// Dòng lệnh `--install` / `--uninstall`.
pub fn cli_install(test: bool, quiet: bool) {
    let res = install(test);
    let msg = match &res {
        Ok(m) => m.clone(),
        Err(e) => {
            let m = tf("inst.fail", &[e.as_str()]);
            log_line(&m);
            m
        }
    };
    if !quiet {
        message_box(&t("inst.title"), &msg, false);
    }
    if res.is_ok() && !test && !quiet {
        // mở bản đã cài
        let _ = Command::new(install_layout(false).exe).spawn();
    }
}

pub fn cli_uninstall(test: bool, quiet: bool) {
    let msg = match uninstall(test, quiet) {
        Ok(m) => m,
        Err(e) => {
            let m = tf("un.fail", &[&e]);
            log_line(&m);
            m
        }
    };
    if !quiet {
        message_box(&t("un.title"), &msg, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dùng tên giá trị riêng "LarmaMic-Test" nên không đụng mục khởi động thật của app.
    #[test]
    fn run_entry_roundtrip_with_quotes() {
        let v = "LarmaMic-Test-Run";
        let exe = r"C:\Program Files\Larma Mic\larma-mic.exe"; // có khoảng trắng: phải được bọc ngoặc kép
        assert!(set_run_entry(v, exe, true));
        assert!(run_entry_exists(v));
        let out = Command::new("reg").args(["query", RUN_KEY, "/v", v]).creation_flags(0x0800_0000).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(text.contains(&format!("\"{exe}\" --background")), "giá trị đã ghi: {text}");
        assert!(set_run_entry(v, exe, false));
        assert!(!run_entry_exists(v));
    }
}
