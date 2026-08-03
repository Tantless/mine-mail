#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, WindowEvent};

static NSIS_PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mine-mail-payload.exe"));
const PRODUCT_NAME: &str = "Mine Mail";
const PRODUCT_SHORTCUT: &str = "Mine Mail.lnk";
const ROOT_INSTALL_DIR_NAME: &str = "MineMail";
const EXPECTED_INSTALL_DIR_ENV: &str = "MINE_MAIL_EXPECTED_INSTALL_DIR";

#[derive(Default)]
struct InstallerRuntime {
    installing: AtomicBool,
    installed_executable: Mutex<Option<PathBuf>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallerInfo {
    version: &'static str,
    default_install_dir: String,
    payload_available: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallResult {
    install_dir: String,
    desktop_shortcut_enabled: bool,
    autostart_enabled: bool,
}

#[derive(Clone, Serialize)]
struct StagePayload {
    label: &'static str,
    message: &'static str,
}

#[tauri::command]
fn installer_info() -> InstallerInfo {
    InstallerInfo {
        version: env!("MINE_MAIL_RELEASE_VERSION"),
        default_install_dir: default_install_dir().to_string_lossy().into_owned(),
        payload_available: !NSIS_PAYLOAD.is_empty(),
    }
}

#[tauri::command]
fn minimize_installer(window: WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
fn close_installer(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if app
        .state::<InstallerRuntime>()
        .installing
        .load(Ordering::SeqCst)
    {
        let _ = window.emit("installer://close-blocked", ());
        return Ok(());
    }
    window.close().map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_install(app: AppHandle, install_dir: String) -> Result<InstallResult, String> {
    let install_dir = validate_install_dir(&install_dir)?;
    let runtime = app.state::<InstallerRuntime>();
    if runtime
        .installing
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("安装已经在进行中。".to_owned());
    }
    drop(runtime);

    let app_for_task = app.clone();
    let install_dir_for_task = install_dir.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        perform_install(&app_for_task, &install_dir_for_task)
    })
    .await
    .map_err(|error| format!("安装任务意外中止：{error}"))
    .and_then(|result| result);

    app.state::<InstallerRuntime>()
        .installing
        .store(false, Ordering::SeqCst);

    match result {
        Ok(executable) => {
            let runtime = app.state::<InstallerRuntime>();
            let mut installed = runtime
                .installed_executable
                .lock()
                .map_err(|_| "无法保存安装结果。".to_owned())?;
            *installed = Some(executable);
            Ok(InstallResult {
                install_dir: install_dir.to_string_lossy().into_owned(),
                desktop_shortcut_enabled: desktop_shortcut_enabled(),
                autostart_enabled: installed
                    .as_ref()
                    .is_some_and(|executable| autostart_enabled(executable).unwrap_or(false)),
            })
        }
        Err(error) => Err(error),
    }
}

#[tauri::command]
fn launch_installed_app(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    let executable = installed_executable(&app)?;

    if !executable.is_file() {
        return Err("Mine Mail 的安装文件不存在，请重新运行安装程序。".to_owned());
    }

    let mut command = Command::new(&executable);
    if let Some(parent) = executable.parent() {
        command.current_dir(parent);
    }
    command
        .spawn()
        .map_err(|error| format!("无法启动 Mine Mail：{error}"))?;
    window.close().map_err(|error| error.to_string())
}

#[tauri::command]
fn set_desktop_shortcut_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let executable = installed_executable(&app)?;
    update_desktop_shortcut(&executable, enabled)?;
    Ok(desktop_shortcut_enabled())
}

#[tauri::command]
fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let executable = installed_executable(&app)?;
    update_autostart(&executable, enabled)?;
    autostart_enabled(&executable)
}

fn installed_executable(app: &AppHandle) -> Result<PathBuf, String> {
    app.state::<InstallerRuntime>()
        .installed_executable
        .lock()
        .map_err(|_| "无法读取安装结果。".to_owned())?
        .clone()
        .ok_or_else(|| "尚未找到已安装的 Mine Mail。".to_owned())
}

fn desktop_shortcut_path() -> Result<PathBuf, String> {
    dirs::desktop_dir()
        .map(|path| path.join(PRODUCT_SHORTCUT))
        .ok_or_else(|| "无法定位当前用户的桌面文件夹。".to_owned())
}

fn start_menu_shortcut_path() -> Result<PathBuf, String> {
    dirs::data_dir()
        .map(|path| {
            path.join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join(PRODUCT_SHORTCUT)
        })
        .ok_or_else(|| "无法定位当前用户的开始菜单文件夹。".to_owned())
}

fn desktop_shortcut_enabled() -> bool {
    desktop_shortcut_path().is_ok_and(|path| path.is_file())
}

fn update_desktop_shortcut(executable: &Path, enabled: bool) -> Result<(), String> {
    if !executable.is_file() {
        return Err("Mine Mail 的安装文件不存在，请重新运行安装程序。".to_owned());
    }

    let desktop_shortcut = desktop_shortcut_path()?;
    if enabled {
        if desktop_shortcut.is_file() {
            return Ok(());
        }
        let start_menu_shortcut = start_menu_shortcut_path()?;
        if !start_menu_shortcut.is_file() {
            return Err("没有找到 Mine Mail 的开始菜单快捷方式。".to_owned());
        }
        fs::copy(&start_menu_shortcut, &desktop_shortcut)
            .map_err(|error| format!("无法创建桌面图标：{error}"))?;
    } else if desktop_shortcut.exists() {
        fs::remove_file(&desktop_shortcut).map_err(|error| format!("无法移除桌面图标：{error}"))?;
    }
    Ok(())
}

fn autolaunch(executable: &Path) -> Result<AutoLaunch, String> {
    let executable = executable
        .to_str()
        .ok_or_else(|| "Mine Mail 的安装路径无法用于开机自启。".to_owned())?;
    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name(PRODUCT_NAME)
        .set_app_path(executable)
        .set_args(&["--background"]);
    builder
        .build()
        .map_err(|error| format!("无法准备开机自启设置：{error}"))
}

fn autostart_enabled(executable: &Path) -> Result<bool, String> {
    autolaunch(executable)?
        .is_enabled()
        .map_err(|error| format!("无法读取开机自启设置：{error}"))
}

fn update_autostart(executable: &Path, enabled: bool) -> Result<(), String> {
    if !executable.is_file() {
        return Err("Mine Mail 的安装文件不存在，请重新运行安装程序。".to_owned());
    }

    let autolaunch = autolaunch(executable)?;
    let current = autolaunch
        .is_enabled()
        .map_err(|error| format!("无法读取开机自启设置：{error}"))?;
    if current == enabled {
        return Ok(());
    }
    if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    }
    .map_err(|error| format!("无法更新开机自启设置：{error}"))
}

fn perform_install(app: &AppHandle, install_dir: &Path) -> Result<PathBuf, String> {
    if NSIS_PAYLOAD.is_empty() {
        return Err("安装程序未包含 Mine Mail 安装载荷。".to_owned());
    }

    emit_stage(app, "准备安装文件", "正在检查安装环境并准备应用文件…");
    prepare_install_dir(install_dir)?;
    let temporary_dir = tempfile::Builder::new()
        .prefix("mine-mail-setup-")
        .tempdir()
        .map_err(|error| format!("无法创建临时安装目录：{error}"))?;
    let payload_path = temporary_dir.path().join("mine-mail-payload.exe");
    fs::write(&payload_path, NSIS_PAYLOAD).map_err(|error| format!("无法释放安装文件：{error}"))?;

    emit_stage(app, "写入应用文件", "正在安装 Mine Mail…");
    let mut install_dir_argument = OsString::from("/D=");
    install_dir_argument.push(install_dir.as_os_str());
    let mut command = Command::new(&payload_path);
    command
        .arg("/S")
        .env(EXPECTED_INSTALL_DIR_ENV, install_dir.as_os_str());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.raw_arg(&install_dir_argument);
    }
    #[cfg(not(windows))]
    command.arg(&install_dir_argument);
    let status = command
        .status()
        .map_err(|error| format!("无法启动内部安装程序：{error}"))?;

    if !status.success() {
        let code = status
            .code()
            .map_or_else(|| "未知".to_owned(), |value| value.to_string());
        return Err(format!("内部安装程序返回了错误代码 {code}。"));
    }

    emit_stage(app, "确认安装结果", "正在完成最后检查…");
    locate_installed_executable(install_dir)
        .ok_or_else(|| "安装程序已结束，但没有在目标目录中找到 Mine Mail。".to_owned())
}

fn prepare_install_dir(install_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(install_dir)
        .map_err(|error| format!("无法创建安装位置，请确认所选磁盘可用且目录可写：{error}"))?;
    tempfile::Builder::new()
        .prefix(".mine-mail-install-check-")
        .tempfile_in(install_dir)
        .map_err(|error| format!("安装位置不可写，请选择其他位置：{error}"))?
        .close()
        .map_err(|error| format!("无法完成安装位置检查，请选择其他位置：{error}"))
}

fn emit_stage(app: &AppHandle, label: &'static str, message: &'static str) {
    let _ = app.emit("installer://stage", StagePayload { label, message });
}

fn validate_install_dir(value: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("请选择安装位置。".to_owned());
    }
    if trimmed.contains(['\r', '\n', '\0', '"']) {
        return Err("安装位置包含无效字符。".to_owned());
    }

    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err("安装位置必须是完整路径。".to_owned());
    }
    if path.is_file() {
        return Err("安装位置指向了一个文件，请选择文件夹。".to_owned());
    }
    if path.parent().is_none() {
        return Ok(path.join(ROOT_INSTALL_DIR_NAME));
    }
    Ok(path)
}

fn default_install_dir() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .map(|path| path.join("AppData").join("Local"))
        })
        .unwrap_or_else(|| PathBuf::from(r"C:\Mine Mail"))
        .join("Mine Mail")
}

fn locate_installed_executable(install_dir: &Path) -> Option<PathBuf> {
    for candidate in ["Mine Mail.exe", "mine-mail-desktop.exe"] {
        let executable = install_dir.join(candidate);
        if executable.is_file() {
            return Some(executable);
        }
    }

    fs::read_dir(install_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
                && !path.file_stem().is_some_and(|stem| {
                    stem.to_string_lossy()
                        .to_ascii_lowercase()
                        .contains("uninstall")
                })
        })
}

fn main() {
    tauri::Builder::default()
        .manage(InstallerRuntime::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            installer_info,
            minimize_installer,
            close_installer,
            start_install,
            launch_installed_app,
            set_desktop_shortcut_enabled,
            set_autostart_enabled,
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window
                    .app_handle()
                    .state::<InstallerRuntime>()
                    .installing
                    .load(Ordering::SeqCst)
            {
                api.prevent_close();
                let _ = window.emit("installer://close-blocked", ());
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run the Mine Mail installer");
}

#[cfg(test)]
mod tests {
    use super::{locate_installed_executable, prepare_install_dir, validate_install_dir};
    use std::fs;

    #[test]
    fn rejects_relative_or_control_character_paths() {
        assert!(validate_install_dir("Mine Mail").is_err());
        assert!(validate_install_dir("C:\\Mine Mail\nother").is_err());
    }

    #[test]
    fn accepts_an_absolute_windows_install_path() {
        let path = validate_install_dir(r"C:\Users\Test\AppData\Local\Mine Mail")
            .expect("absolute Windows path should be accepted");
        assert!(path.is_absolute());
    }

    #[test]
    fn places_a_drive_root_install_inside_a_minemail_directory() {
        let path = validate_install_dir(r"D:\").expect("drive root should be normalized");
        assert_eq!(path, std::path::PathBuf::from(r"D:\MineMail"));
    }

    #[test]
    fn creates_and_checks_a_missing_install_directory() {
        let parent = tempfile::tempdir().expect("temporary parent directory");
        let install_dir = parent.path().join("MineMail");

        prepare_install_dir(&install_dir).expect("install directory should be prepared");

        assert!(install_dir.is_dir());
        assert_eq!(
            fs::read_dir(&install_dir)
                .expect("prepared install directory")
                .count(),
            0
        );
    }

    #[test]
    fn locates_the_installed_app_without_selecting_the_uninstaller() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(directory.path().join("uninstall.exe"), []).expect("uninstaller fixture");
        let app = directory.path().join("Mine Mail.exe");
        fs::write(&app, []).expect("app fixture");

        assert_eq!(locate_installed_executable(directory.path()), Some(app));
    }
}
