//! `os::env` on Windows: the environment guard ([OS/env]; X-F6 allow-lists and refusals, X-F5's "no downgrade" at
//! `init`).
//!
//! - **Allowed:** NTFS only (`ZeroFill` extents), crash-gated on GT1/GT3 and GT4 evidence ([OS/env §3]).
//! - **Refused:** ReFS and Dev Drive, FAT, FAT32, exFAT and any other name (`FileSystem`); `DRIVE_REMOTE` (`Network`);
//!   a final path in `\\?\UNC\` form, which covers `\\wsl$`, `\\wsl.localhost` and mapped network drives (`Unc`; a `subst`
//!   drive of a local folder resolves locally and is allowed); cloud-managed folders (`Cloud(WindowsCloudFiles)`);
//!   `DRIVE_RAMDISK` (`Volatile`).
//! - **At every open** (`ClassifyDepth::Open`): one volume query plus cheap checks on the store directory's own handle
//!   ([OS/env §4.1]). **At `init`, `restore` and `doctor`** (`Full`): also `CfGetSyncRootInfoByPath`, the cloud
//!   attributes of every ancestor (read from the parent's directory entry, never by opening the ancestor), and the
//!   `OneDrive*` environment folders ([OS/env §5] step 1).
//! - **The full probe** ([OS/env §5]) performs the durable write, the directory flushes, the byte-range lock at 2^62
//!   and the no-replace rename in `tmp/`; a refused call refuses the store and nothing is downgraded.
//! - **The OS version** comes from `RtlGetVersion` ([OS/env §6]).

#![allow(unsafe_code)]

use std::sync::OnceLock;

use moirai_vfs::{
    Classification, ClassifyDepth, CloudKind, Entropy, EnvGuard, EnvWarning, ExtentMethod, FsKind,
    FsName, OS_SHARE_RETRY_MS, OsCode, OsVersion, ProbeOutcome, ProbeReport, Refusal, RelPath,
    ShareRetry, StoreFs, StoreVolume, SyncKind, VfsError, VfsErrorKind,
};
use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_Device_Interface_List_SizeW,
    CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_PropertyW, CM_LOCATE_DEVNODE_NORMAL,
    CM_Locate_DevNodeW, CM_Open_DevNode_Key, CM_REGISTRY_HARDWARE, CR_SUCCESS,
    RegDisposition_OpenExisting,
};
use windows_sys::Win32::Devices::Properties::{DEVPKEY_Device_InstanceId, DEVPROPTYPE};
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::CloudFilters::{CF_SYNC_ROOT_INFO_BASIC, CfGetSyncRootInfoByPath};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FileAttributeTagInfo, FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW,
    GetDriveTypeW, GetVolumeInformationByHandleW, GetVolumePathNameW, OPEN_EXISTING,
    WIN32_FIND_DATAW,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{
    GUID_DEVINTERFACE_DISK, IOCTL_STORAGE_GET_DEVICE_NUMBER, STORAGE_DEVICE_NUMBER,
};
use windows_sys::Win32::System::Registry::{
    HKEY, KEY_READ, RRF_RT_REG_DWORD, RegCloseKey, RegGetValueW,
};
use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows_sys::Win32::System::SystemServices::FILE_READ_ONLY_VOLUME;
use windows_sys::Win32::System::WindowsProgramming::{
    DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
};

use super::fs::{OsFile, OsRoot, OsVfs};
use super::path::{canonical_abs, rewrite_final};
use super::sys::{self, Domain, create_file, error, file_info, last, raw};

/// The refusal floor: Windows 10 1803, build 17134 ([OS/env §6]).
const FLOOR: OsVersion = OsVersion {
    major: 10,
    minor: 0,
    build: 17_134,
};
/// Windows 11's first build: supported and tested from here ([OS/env §6]).
const WINDOWS_11: u32 = 22_000;

/// The cloud-only attributes of [OS/env §4.1] step 4.
const CLOUD_ATTRS: u32 =
    FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_OFFLINE;

/// `IO_REPARSE_TAG_CLOUD` and its variants: `(tag & !0x0000_F000) == 0x9000_001A`.
pub(crate) fn is_cloud_tag(tag: u32) -> bool {
    tag & !0x0000_F000 == 0x9000_001A
}

/// `true` if an entry's attributes (and reparse tag) mark it cloud-managed.
fn cloud_marked(attrs: u32, tag: u32) -> bool {
    attrs & CLOUD_ATTRS != 0 || (attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 && is_cloud_tag(tag))
}

/// The file-system name and flags of the volume holding `h` (`GetVolumeInformationByHandleW`).
pub(crate) fn volume_name_and_flags(h: HANDLE) -> Result<(String, u32), VfsError> {
    let mut name = [0u16; 261];
    let mut flags = 0u32;
    // SAFETY: `name` is writable for 261 units; `flags` is a live local; the other buffers are null with size 0.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            h,
            core::ptr::null_mut(),
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &mut flags,
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if ok == 0 {
        return Err(last(Domain::Store, "GetVolumeInformationByHandleW"));
    }
    let n = name.iter().position(|&u| u == 0).unwrap_or(name.len());
    Ok((String::from_utf16_lossy(&name[..n]), flags))
}

/// The volume root of a `\\?\` path (with trailing `\`), from `GetVolumePathNameW`, NUL-terminated.
fn volume_root(path_z: &[u16]) -> Result<Vec<u16>, VfsError> {
    let mut buf: Vec<u16> = vec![0; path_z.len() + 2];
    // SAFETY: `path_z` is NUL-terminated; `buf` is writable for its length, which is enough for any prefix of the path.
    if unsafe { GetVolumePathNameW(path_z.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0 {
        return Err(last(Domain::Store, "GetVolumePathNameW"));
    }
    let n = buf.iter().position(|&u| u == 0).unwrap_or(buf.len());
    buf.truncate(n + 1);
    Ok(buf)
}

/// `GetDriveTypeW` of the volume holding the `\\?\` path `path_z` (NUL-terminated), whose volume root comes from
/// `GetVolumePathNameW` ([OS/env §4.1] step 3; [OS/project §4.3] for the network-redirector row).
pub(crate) fn drive_type(path_z: &[u16]) -> Result<u32, VfsError> {
    let root = volume_root(path_z)?;
    // SAFETY: `root` is NUL-terminated.
    Ok(unsafe { GetDriveTypeW(root.as_ptr()) })
}

/// The attributes and reparse tag of the entry `path_z` names, read from its parent's directory entry
/// (`FindFirstFileExW(FindExInfoBasic)`), so the entry itself is never opened.
fn entry_attrs(path_z: &[u16]) -> Option<(u32, u32)> {
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: `path_z` is NUL-terminated; `data` is a live `WIN32_FIND_DATAW`; no filter, no flags.
    let h = unsafe {
        FindFirstFileExW(
            path_z.as_ptr(),
            FindExInfoBasic,
            (&mut data as *mut WIN32_FIND_DATAW).cast(),
            FindExSearchNameMatch,
            core::ptr::null(),
            0,
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return None;
    }
    // SAFETY: `h` is the search handle just returned, closed once.
    unsafe { FindClose(h) };
    Some((data.dwFileAttributes, data.dwReserved0))
}

/// `true` for a `\\?\UNC\` path (no NUL), compared ignoring ASCII case ([OS/env §4.1] step 1).
pub(crate) fn is_unc(path: &[u16]) -> bool {
    let unc = "\\\\?\\UNC\\".encode_utf16();
    path.len() >= 8
        && path
            .iter()
            .zip(unc)
            .all(|(&a, b)| a == b || (a >= 0x61 && a - 0x20 == b))
}

/// The plain Win32 form of a `\\?\` path (`X:\…`, `\\server\share\…`), NUL-terminated.
fn plain_z(verbatim: &[u16]) -> Vec<u16> {
    let s = String::from_utf16_lossy(verbatim);
    let plain = if let Some(r) = s.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{r}")
    } else {
        s.strip_prefix("\\\\?\\").unwrap_or(&s).to_owned()
    };
    sys::wide_z(plain.as_ref())
}

/// The full-depth cloud checks of [OS/env §5] step 1 on a store directory's `\\?\` path.
fn cloud_full(path: &[u16]) -> bool {
    // A registered Cloud Files sync root (OneDrive or any other provider) at or above the path.
    let plain = plain_z(path);
    let mut info = [0u64; 2];
    let mut ret = 0u32;
    // SAFETY: `plain` is NUL-terminated; `info` is writable for 16 bytes (a `CF_SYNC_ROOT_BASIC_INFO` is 8).
    let hr = unsafe {
        CfGetSyncRootInfoByPath(
            plain.as_ptr(),
            CF_SYNC_ROOT_INFO_BASIC,
            info.as_mut_ptr().cast(),
            16,
            &mut ret,
        )
    };
    if hr >= 0 {
        return true;
    }
    // The cloud attributes on every ancestor up to the volume root.
    // (A volume root has no directory entry: `FindFirstFileExW` fails on it, and it is skipped.)
    let mut p = path.to_vec();
    while let Some(parent) = super::fs::parent_z(&p) {
        if let Some((attrs, tag)) = entry_attrs(&parent)
            && cloud_marked(attrs, tag)
        {
            return true;
        }
        p = parent[..parent.len() - 1].to_vec();
    }
    // A folder named by `OneDrive`, `OneDriveConsumer` or `OneDriveCommercial`.
    let Some(text) = rewrite_final(path) else {
        return false;
    };
    let text = text.to_lowercase();
    ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]
        .iter()
        .any(|var| {
            let Some(dir) = std::env::var_os(var).filter(|d| !d.is_empty()) else {
                return false;
            };
            let Ok(abs) = canonical_abs(std::path::Path::new(&dir)) else {
                return false;
            };
            let d = abs.as_str().to_lowercase();
            text == d || text.starts_with(&format!("{}/", d.trim_end_matches('/')))
        })
}

/// The running OS version from `RtlGetVersion`, read once.
fn os_version() -> OsVersion {
    static V: OnceLock<OsVersion> = OnceLock::new();
    *V.get_or_init(|| {
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: core::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is a live `OSVERSIONINFOW` whose size field is set; the call always succeeds for it.
        unsafe { RtlGetVersion(&mut info) };
        OsVersion {
            major: info.dwMajorVersion,
            minor: info.dwMinorVersion,
            build: info.dwBuildNumber,
        }
    })
}

/// `IOCTL_STORAGE_GET_DEVICE_NUMBER` on a device path (NUL-terminated), opened with no access.
fn device_number(path_z: &[u16]) -> Option<STORAGE_DEVICE_NUMBER> {
    let h = create_file(
        path_z,
        0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        OPEN_EXISTING,
        0,
    )
    .ok()?;
    let mut n = STORAGE_DEVICE_NUMBER::default();
    let mut ret = 0u32;
    // SAFETY: `n` is a live output buffer of its size; no input; synchronous (no OVERLAPPED).
    let ok = unsafe {
        DeviceIoControl(
            raw(&h),
            IOCTL_STORAGE_GET_DEVICE_NUMBER,
            core::ptr::null(),
            0,
            (&mut n as *mut STORAGE_DEVICE_NUMBER).cast(),
            core::mem::size_of::<STORAGE_DEVICE_NUMBER>() as u32,
            &mut ret,
            core::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(n)
}

/// Whether the disk holding the drive-letter volume `volume_root` (`\\?\X:\`) has "turn off Windows write-cache buffer
/// flushing" set: `CacheIsPowerProtected` ≠ 0 under the disk's `Device Parameters\Disk` key ([OS/env §8]). `None` when
/// the disk or its key cannot be found (a volume spanning disks, a UNC or mounted-folder volume).
fn flushing_disabled(volume_root: &[u16]) -> Option<bool> {
    let s = String::from_utf16_lossy(&volume_root[..volume_root.len().saturating_sub(1)]);
    let letter = s.strip_prefix("\\\\?\\")?.strip_suffix(":\\")?;
    let vol = device_number(&sys::wide_z(format!("\\\\.\\{letter}:").as_ref()))?;
    let mut len = 0u32;
    // SAFETY: `len` is a live local; the class GUID is a static; no device filter.
    let cr = unsafe {
        CM_Get_Device_Interface_List_SizeW(
            &mut len,
            &GUID_DEVINTERFACE_DISK,
            core::ptr::null(),
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    };
    if cr != CR_SUCCESS || len == 0 {
        return None;
    }
    let mut list: Vec<u16> = vec![0; len as usize];
    // SAFETY: `list` is writable for `len` units.
    let cr = unsafe {
        CM_Get_Device_Interface_ListW(
            &GUID_DEVINTERFACE_DISK,
            core::ptr::null(),
            list.as_mut_ptr(),
            len,
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    };
    if cr != CR_SUCCESS {
        return None;
    }
    for iface in list.split(|&u| u == 0).filter(|s| !s.is_empty()) {
        let iface_z = sys::with_nul(iface);
        match device_number(&iface_z) {
            Some(d) if d.DeviceType == vol.DeviceType && d.DeviceNumber == vol.DeviceNumber => {}
            _ => continue,
        }
        let mut ty: DEVPROPTYPE = 0;
        let mut id = [0u16; 512];
        let mut size = (id.len() * 2) as u32;
        // SAFETY: `iface_z` is NUL-terminated; `id` is writable for `size` bytes; the key is a static.
        let cr = unsafe {
            CM_Get_Device_Interface_PropertyW(
                iface_z.as_ptr(),
                &DEVPKEY_Device_InstanceId,
                &mut ty,
                id.as_mut_ptr().cast(),
                &mut size,
                0,
            )
        };
        if cr != CR_SUCCESS {
            return None;
        }
        let mut devinst = 0u32;
        // SAFETY: `id` holds the NUL-terminated instance id just read.
        if unsafe { CM_Locate_DevNodeW(&mut devinst, id.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) }
            != CR_SUCCESS
        {
            return None;
        }
        let mut key: HKEY = core::ptr::null_mut();
        // SAFETY: `key` is a live local; the hardware key ("Device Parameters") is opened for reading only.
        let cr = unsafe {
            CM_Open_DevNode_Key(
                devinst,
                KEY_READ,
                0,
                RegDisposition_OpenExisting,
                &mut key,
                CM_REGISTRY_HARDWARE,
            )
        };
        if cr != CR_SUCCESS {
            return None;
        }
        let (sub, value) = (
            sys::wide_z("Disk".as_ref()),
            sys::wide_z("CacheIsPowerProtected".as_ref()),
        );
        let mut data = 0u32;
        let mut dsize = 4u32;
        // SAFETY: `key` is open; the names are NUL-terminated; `data` is writable for 4 bytes.
        let rc = unsafe {
            RegGetValueW(
                key,
                sub.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                core::ptr::null_mut(),
                (&mut data as *mut u32).cast(),
                &mut dsize,
            )
        };
        // SAFETY: `key` was opened above and is closed once.
        unsafe { RegCloseKey(key) };
        // An absent value is the default: flushing on.
        return Some(rc == 0 && data != 0);
    }
    None
}

/// The store's `tmp/` directory ([F02 §5.3]).
const TMP: RelPath<'static> = RelPath::literal("tmp");

/// The clean-up's share-retry bound: HOLE(OS-share-retry-ms) ([`moirai_vfs::OS_SHARE_RETRY_MS`], [OS/fs §6.3]).
/// Defender or the indexer may hold a freshly written probe file for a moment (fault-model W3).
const CLEANUP_RETRY: ShareRetry = ShareRetry::Bounded {
    total_ms: OS_SHARE_RETRY_MS,
};

/// A probe file's name `tmp/probe.<nonce>` ([OS/env §5]; [F02 §5.3, §6.3] `tmp-entry`: the word `probe` and the nonce
/// in decimal).
fn probe_name(nonce: u64) -> String {
    format!("tmp/probe.{nonce}")
}

/// The `RelPath` of a probe name.
fn rel(name: &str) -> RelPath<'_> {
    RelPath::new(name).expect("os::env: `tmp/probe.<u64>` is a valid RelPath")
}

/// One nonce: a `u64` drawn from `Entropy::fill_random` with one call of exactly its width ([OS/README §4.6]).
fn draw(v: &OsVfs) -> u64 {
    let mut b = [0u8; 8];
    v.fill_random(&mut b);
    u64::from_le_bytes(b)
}

/// `create_new` of a fresh `tmp/probe.<nonce>`, the nonce drawn again while the name exists and never equal to one of
/// `taken`; returns the nonce and the open file, and records the name in `made` for the clean-up.
fn create_probe(
    v: &OsVfs,
    store: &OsRoot,
    taken: &[u64],
    made: &mut Vec<String>,
) -> Result<(u64, OsFile), VfsError> {
    loop {
        let n = draw(v);
        if taken.contains(&n) {
            continue;
        }
        let name = probe_name(n);
        match v.create_new(store, rel(&name)) {
            Ok(f) => {
                made.push(name);
                return Ok((n, f));
            }
            Err(e) if e.kind == VfsErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

/// Steps 3–5 of [OS/env §5] with the nonce names a, b and c; `Ok(Some(refusal))` for a refused call. Every name the probe
/// creates is pushed to `made`, so the clean-up (step 6) removes exactly the probe's own files and never a concurrent
/// prober's.
fn probe_calls(
    v: &OsVfs,
    store: &OsRoot,
    made: &mut Vec<String>,
) -> Result<Option<Refusal>, VfsError> {
    // 3. The durable-write probe on `tmp/probe.a`; a durability failure is a refusal here, not a `fail_stop` (open
    //    point 7).
    let (a, f) = create_probe(v, store, &[], made)?;
    let page: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    v.write_at(&f, 0, &page)?;
    let flushed = v
        .sync(&f, SyncKind::Data)
        .and_then(|()| v.sync(&f, SyncKind::DataAndMeta))
        .and_then(|()| v.sync_dir(store, Some(TMP)))
        .and_then(|()| v.sync_dir(store, None));
    drop(f);
    if let Err(df) = flushed {
        return Ok(Some(Refusal::NoDurableFlush {
            call: df.call,
            os: df.os,
        }));
    }
    let name_a = probe_name(a);
    // 4. The lock probe at 2^62 on `tmp/probe.a` (`LOCK` does not exist yet); "busy" is not a refusal.
    if let Err(e) = super::lock::probe_byte_locks(store, rel(&name_a)) {
        return Ok(Some(Refusal::NoByteLocks { os: e.os }));
    }
    // 5. The rename probe: `create_new(tmp/probe.c)`; `tmp/probe.a → tmp/probe.b` must succeed (b drawn again while
    //    its name exists); `tmp/probe.b → tmp/probe.c` must fail with `AlreadyExists`.
    let (c, fc) = create_probe(v, store, &[a], made)?;
    drop(fc);
    let name_c = probe_name(c);
    let name_b = loop {
        let b = draw(v);
        if b == a || b == c {
            continue;
        }
        let name_b = probe_name(b);
        match v.rename_noreplace(store, rel(&name_a), store, rel(&name_b), ShareRetry::None) {
            Ok(()) => {
                made.push(name_b.clone());
                break name_b;
            }
            // Another entry holds the name b: draw again.
            Err(e) if e.kind == VfsErrorKind::AlreadyExists => {}
            Err(e) if e.kind == VfsErrorKind::Unsupported => {
                return Ok(Some(Refusal::NoNoReplaceRename { os: e.os }));
            }
            Err(e) => return Err(e),
        }
    };
    match v.rename_noreplace(store, rel(&name_b), store, rel(&name_c), ShareRetry::None) {
        Ok(()) => Ok(Some(Refusal::NoNoReplaceRename { os: OsCode::NONE })),
        Err(e) if e.kind == VfsErrorKind::AlreadyExists => Ok(None),
        Err(e) if e.kind == VfsErrorKind::Unsupported => {
            Ok(Some(Refusal::NoNoReplaceRename { os: e.os }))
        }
        Err(e) => Err(e),
    }
}

impl EnvGuard for OsVfs {
    fn classify(&self, store: &OsRoot, depth: ClassifyDepth) -> Result<Classification, VfsError> {
        let h = store.raw();
        // [OS/env §4.1] step 1: a final path in UNC form (captured at `open_root`) needs no OS call, so it runs first,
        // and a UNC or network root whose volume query fails is still `Refused(Unc)`, not an error.
        let path = store.path();
        if is_unc(path) {
            return Ok(Classification::Refused(Refusal::Unc));
        }
        // 2. The file-system name and the volume flags.
        let (name, flags) = volume_name_and_flags(h)?;
        let fs_name = FsName::lossy(name.as_bytes());
        let read_only = flags & FILE_READ_ONLY_VOLUME != 0;
        // 3. The drive type of the volume root.
        let removable = match drive_type(&sys::with_nul(path))? {
            DRIVE_REMOTE => return Ok(Classification::Refused(Refusal::Network { name: fs_name })),
            DRIVE_RAMDISK => {
                return Ok(Classification::Refused(Refusal::Volatile { name: fs_name }));
            }
            DRIVE_REMOVABLE => true,
            _ => false,
        };
        // 4. Cloud attributes or a cloud reparse tag on the store directory.
        let tag = file_info(h, FileAttributeTagInfo, FILE_ATTRIBUTE_TAG_INFO::default())
            .map_err(|e| error(e, Domain::Store, "GetFileInformationByHandleEx"))?;
        if cloud_marked(tag.FileAttributes, tag.ReparseTag)
            || (depth == ClassifyDepth::Full && cloud_full(path))
        {
            return Ok(Classification::Refused(Refusal::Cloud {
                kind: CloudKind::WindowsCloudFiles,
            }));
        }
        // 5. The allow-list: exactly `NTFS`.
        if name == "NTFS" {
            Ok(Classification::Local(StoreVolume {
                fs: FsKind::Ntfs,
                extent_method: ExtentMethod::ZeroFill,
                read_only,
                removable,
            }))
        } else {
            Ok(Classification::Refused(Refusal::FileSystem {
                name: fs_name,
            }))
        }
    }

    fn probe_store(&self, store: &OsRoot) -> Result<ProbeOutcome, VfsError> {
        // 1. Full classification.
        let volume = match self.classify(store, ClassifyDepth::Full)? {
            Classification::Local(v) => v,
            Classification::Refused(r) => return Ok(ProbeOutcome::Refused(r)),
        };
        // 2. The OS version.
        let os = match self.check_os_version() {
            Ok(v) => v,
            Err(r) => return Ok(ProbeOutcome::Refused(r)),
        };
        // `tmp/` must exist: its absence is not about the location.
        self.path_identity(store, TMP)?;
        // Probe files another probe or a crash left behind are not touched: they are `probe.<nonce>` names of
        // [F02 §6.3]'s grammar, which the orphan sweep removes ([F02 §5.3]).
        let mut made: Vec<String> = Vec::with_capacity(3);
        let r = probe_calls(self, store, &mut made);
        // 6. Clean-up, whatever the outcome: unlink this probe's own files (a renamed one is absent, which is fine), then
        // `sync_dir(tmp)`. A clean-up failure never turns `Admitted` into an error or a refusal ([OS/env §5] step 6,
        // spec sync 2a): steps 3–5 have already proved the location; a file whose unlink still fails after the bound
        // (Defender or the indexer holding it) stays for the orphan sweep, and a failed `sync_dir(tmp)` only leaves the
        // unlinks pending, which a crash may undo with the same result. Neither goes to `fail_stop`.
        for name in &made {
            let _ = self.unlink(store, rel(name), CLEANUP_RETRY);
        }
        let _ = self.sync_dir(store, Some(TMP));
        Ok(match r? {
            Some(refusal) => ProbeOutcome::Refused(refusal),
            None => ProbeOutcome::Admitted(ProbeReport { volume, os }),
        })
    }

    fn check_os_version(&self) -> Result<OsVersion, Refusal> {
        let v = os_version();
        if v < FLOOR {
            Err(Refusal::OsTooOld {
                found: v,
                minimum: FLOOR,
            })
        } else {
            Ok(v)
        }
    }

    fn doctor_warnings(&self, store: &OsRoot) -> Vec<EnvWarning> {
        let mut out = Vec::new();
        let v = os_version();
        if v >= FLOOR && v.major == 10 && v.build < WINDOWS_11 {
            out.push(EnvWarning::UntestedOsRelease { found: v });
        }
        if let Ok(Classification::Local(vol)) = self.classify(store, ClassifyDepth::Open)
            && vol.removable
        {
            out.push(EnvWarning::RemovableDrive);
        }
        if let Ok(root) = volume_root(&sys::with_nul(store.path()))
            && flushing_disabled(&root) == Some(true)
        {
            out.push(EnvWarning::FlushingDisabled);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_tags_and_attributes() {
        assert!(is_cloud_tag(0x9000_001A));
        assert!(is_cloud_tag(0x9000_101A));
        assert!(is_cloud_tag(0x9000_F01A));
        assert!(!is_cloud_tag(0xA000_000C), "a symlink");
        assert!(cloud_marked(FILE_ATTRIBUTE_RECALL_ON_OPEN, 0));
        assert!(cloud_marked(FILE_ATTRIBUTE_OFFLINE, 0));
        assert!(
            !cloud_marked(0x10, 0x9000_001A),
            "a tag without the reparse attribute"
        );
        assert!(cloud_marked(FILE_ATTRIBUTE_REPARSE_POINT, 0x9000_301A));
    }

    #[test]
    fn this_os_passes_the_floor() {
        let v = OsVfs.check_os_version().unwrap();
        assert!(v >= FLOOR);
        assert_eq!(v.major, 10);
    }

    #[test]
    fn plain_forms() {
        let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let p = |v: Vec<u16>| String::from_utf16(&v[..v.len() - 1]).unwrap();
        assert_eq!(p(plain_z(&w("\\\\?\\D:\\x"))), "D:\\x");
        assert_eq!(p(plain_z(&w("\\\\?\\UNC\\s\\h"))), "\\\\s\\h");
        // The UNC test of [OS/env §4.1] step 1, which `classify` runs before any OS call.
        assert!(is_unc(&w("\\\\?\\UNC\\wsl$\\Ubuntu\\home")));
        assert!(is_unc(&w("\\\\?\\unc\\srv\\share")));
        assert!(!is_unc(&w("\\\\?\\D:\\UNC\\x")));
        assert!(!is_unc(&w("\\\\?\\UN")));
    }

    #[test]
    fn probe_names_follow_the_tmp_entry_grammar() {
        // [F02 §6.3] `tmp-entry = tmp-word "." u64dec`: the word `probe` and the nonce in decimal, no leading zeros.
        assert_eq!(probe_name(0), "tmp/probe.0");
        assert_eq!(probe_name(u64::MAX), "tmp/probe.18446744073709551615");
        assert_eq!(rel(&probe_name(42)).as_str(), "tmp/probe.42");
        let (a, b) = (draw(&OsVfs), draw(&OsVfs));
        assert_ne!(a, b, "two draws of 64 random bits");
    }
}
