//! 系统文件夹选择框。用 Windows 的通用文件对话框，避免额外依赖。

#[cfg(windows)]
pub fn pick_folder(title: &str) -> Result<Option<String>, String> {
    use std::ffi::c_void;
    use std::sync::mpsc;

    use windows::core::{HSTRING, HRESULT};
    use windows::Win32::Foundation::ERROR_CANCELLED;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };

    let title = title.to_string();
    let (sender, receiver) = mpsc::channel::<Result<Option<String>, String>>();

    /* 对话框需要 STA 线程，放到独立线程里弹，避免阻塞调用方 */
    std::thread::spawn(move || {
        let result = (|| -> Result<Option<String>, String> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                    .ok()
                    .map_err(|error| error.to_string())?;

                let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL)
                    .map_err(|error| error.to_string())?;

                let options = dialog.GetOptions().map_err(|error| error.to_string())?;
                dialog
                    .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
                    .map_err(|error| error.to_string())?;
                dialog
                    .SetTitle(&HSTRING::from(title.as_str()))
                    .map_err(|error| error.to_string())?;

                let shown = dialog.Show(None);
                if let Err(error) = shown {
                    let cancelled: HRESULT = ERROR_CANCELLED.to_hresult();
                    CoUninitialize();
                    if error.code() == cancelled {
                        return Ok(None);
                    }
                    return Err(error.to_string());
                }

                let item = dialog.GetResult().map_err(|error| error.to_string())?;
                let raw = item
                    .GetDisplayName(SIGDN_FILESYSPATH)
                    .map_err(|error| error.to_string())?;
                let path = raw.to_string().map_err(|error| error.to_string())?;
                CoUninitialize();
                let _ = &path as *const _ as *const c_void;
                Ok(Some(path))
            }
        })();

        let _ = sender.send(result);
    });

    receiver
        .recv()
        .map_err(|_| "选择框未返回结果".to_string())?
}

#[cfg(not(windows))]
pub fn pick_folder(_title: &str) -> Result<Option<String>, String> {
    Err("当前平台不支持文件夹选择".to_string())
}
