//! One same-directory Windows rename with live-reader semantics. No retry,
//! copy-over-live-file fallback, ACL bypass or read-only/share-lock override.
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::{
    ffi::OsStrExt,
    fs::OpenOptionsExt,
    io::{AsRawHandle, FromRawHandle},
};
use std::path::Path;
use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileRenameInfoEx, ReOpenFile,
        SetFileAttributesW, SetFileInformationByHandle,
    },
};

pub(super) fn persist(mut pending: tempfile::NamedTempFile, destination: &Path) -> io::Result<()> {
    // POSIX rename permits old handles to keep reading the replaced object.
    // First require ordinary DELETE access to any existing directory entry:
    // an owner's non-delete-sharing handle must still refuse replacement.
    let _target = match OpenOptions::new()
        .access_mode(DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(destination)
    {
        Ok(file) => Some(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    // Reopen the already-owned staging object, not a new path lookup. Rename
    // needs DELETE access; the original tempfile handle remains owned by pending.
    let handle = unsafe {
        ReOpenFile(
            pending.as_file().as_raw_handle(),
            DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: ReOpenFile returned a new, valid handle; File closes it exactly once.
    let rename_file = unsafe { File::from_raw_handle(handle) };
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let filename = destination.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Missing destination filename")
    })?;
    let absolute = parent.canonicalize()?.join(filename);
    let mut name: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    if name.len() > 32767 || name.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid Windows publication path",
        ));
    }
    let name_bytes = u32::try_from(name.len() * 2).map_err(|_| io::ErrorKind::InvalidInput)?;
    name.push(0);
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let size = offset + name.len() * 2;
    // usize storage aligns the native pointer/union structure on both Windows
    // pointer widths; the trailing UTF-16 filename is copied into its byte tail.
    let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: buffer is aligned, initialized and large enough for the header
    // and bounded filename. No references to its one-element tail are extended.
    unsafe {
        (*info).Anonymous.Flags = 0x1 | 0x2; // replace-if-existing + POSIX, nothing else
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength = name_bytes;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            buffer.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
            name.len(),
        );
    }
    // Match tempfile's persistence policy: clear only our staging file's
    // temporary attribute before publication. Never modify destination attrs.
    let stage: Vec<u16> = pending
        .path()
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe { SetFileAttributesW(stage.as_ptr(), FILE_ATTRIBUTE_NORMAL) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: rename_file owns the staging handle; info/size describe the live
    // initialized buffer. Unsupported filesystems/OS versions fail closed.
    if unsafe {
        SetFileInformationByHandle(
            rename_file.as_raw_handle(),
            FileRenameInfoEx,
            info.cast(),
            u32::try_from(size).map_err(|_| io::ErrorKind::InvalidInput)?,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // The old staging name no longer belongs to us. Do not let TempPath drop
    // remove a different file someone might subsequently create at that name.
    pending.disable_cleanup(true);
    Ok(())
}
