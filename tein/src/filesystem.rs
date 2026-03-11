//! `(tein filesystem)` — sandbox-safe filesystem operations.
//!
//! provides real implementations for:
//! - `file-exists?` — checks if path exists (read policy)
//! - `delete-file` — removes a file (write policy)
//! - `file-directory?` — checks if path is a directory (read policy)
//! - `file-regular?` — checks if path is a regular file (read policy)
//! - `file-link?` — checks if path is a symbolic link (read policy)
//! - `file-size` — returns file size in bytes (read policy)
//! - `directory-files` — lists directory contents (read policy)
//! - `create-directory` — creates a directory (write policy)
//! - `delete-directory` — removes an empty directory (write policy)
//! - `rename-file` — renames/moves a file (write policy)
//! - `current-directory` — returns current working directory (no policy check)
//!
//! all remaining `chibi/filesystem` exports are deferred scheme stubs in
//! `filesystem.scm` that raise "not implemented" errors at call time.
//!
//! sandbox integration: all real functions check `IS_SANDBOXED` + `FsPolicy`
//! via [`check_fs_access`]. read operations check read policy; write operations
//! check write policy.

use std::ffi::CString;

use crate::context::{FsAccess, check_fs_access, extract_string_arg};
use crate::ffi;

// --- helpers ---

/// check read access; returns `Ok(())` or `Err(sexp)` with a scheme exception.
unsafe fn check_read(ctx: ffi::sexp, path: &str, _fn_name: &str) -> Result<(), ffi::sexp> {
    if !check_fs_access(path, FsAccess::Read) {
        let msg = format!("[sandbox:file] {} (read not permitted)", path);
        let c_msg = CString::new(msg.as_str()).unwrap_or_default();
        unsafe {
            return Err(ffi::make_error(
                ctx,
                c_msg.as_ptr(),
                msg.len() as ffi::sexp_sint_t,
            ));
        }
    }
    Ok(())
}

/// check write access; returns `Ok(())` or `Err(sexp)` with a scheme exception.
unsafe fn check_write(ctx: ffi::sexp, path: &str, _fn_name: &str) -> Result<(), ffi::sexp> {
    if !check_fs_access(path, FsAccess::Write) {
        let msg = format!("[sandbox:file] {} (write not permitted)", path);
        let c_msg = CString::new(msg.as_str()).unwrap_or_default();
        unsafe {
            return Err(ffi::make_error(
                ctx,
                c_msg.as_ptr(),
                msg.len() as ffi::sexp_sint_t,
            ));
        }
    }
    Ok(())
}

/// make a scheme error from a formatted message string.
unsafe fn make_err(ctx: ffi::sexp, msg: &str) -> ffi::sexp {
    let c_msg = CString::new(msg).unwrap_or_default();
    unsafe { ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t) }
}

// --- trampolines ---

/// `file-exists?`: checks FsPolicy read access, returns boolean.
unsafe extern "C" fn file_exists_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "file-exists?") {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = check_read(ctx, path, "file-exists?") {
            return e;
        }
        if std::path::Path::new(path).exists() {
            ffi::get_true()
        } else {
            ffi::get_false()
        }
    }
}

/// `delete-file`: checks FsPolicy write access, removes file.
unsafe extern "C" fn delete_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "delete-file") {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = check_write(ctx, path, "delete-file") {
            return e;
        }
        match std::fs::remove_file(path) {
            Ok(()) => ffi::get_void(),
            Err(e) => make_err(ctx, &format!("delete-file: {}", e)),
        }
    }
}

/// `file-directory?`: checks FsPolicy read access, returns boolean.
unsafe extern "C" fn file_directory_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "file-directory?") {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = check_read(ctx, path, "file-directory?") {
            return e;
        }
        if std::path::Path::new(path).is_dir() {
            ffi::get_true()
        } else {
            ffi::get_false()
        }
    }
}

/// `file-regular?`: checks FsPolicy read access, returns boolean.
unsafe extern "C" fn file_regular_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "file-regular?") {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = check_read(ctx, path, "file-regular?") {
            return e;
        }
        if std::path::Path::new(path).is_file() {
            ffi::get_true()
        } else {
            ffi::get_false()
        }
    }
}

/// `file-link?`: checks FsPolicy read access, returns boolean.
unsafe extern "C" fn file_link_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "file-link?") {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = check_read(ctx, path, "file-link?") {
            return e;
        }
        if std::path::Path::new(path).is_symlink() {
            ffi::get_true()
        } else {
            ffi::get_false()
        }
    }
}

/// `file-size`: checks FsPolicy read access, returns file size in bytes.
unsafe extern "C" fn file_size_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "file-size") {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = check_read(ctx, path, "file-size") {
            return e;
        }
        match std::fs::metadata(path) {
            Ok(m) => ffi::sexp_make_fixnum(m.len() as ffi::sexp_sint_t),
            Err(e) => make_err(ctx, &format!("file-size: {}", e)),
        }
    }
}

/// `directory-files`: checks FsPolicy read access, returns list of filenames.
unsafe extern "C" fn directory_files_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "directory-files") {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = check_read(ctx, path, "directory-files") {
            return e;
        }
        let entries = match std::fs::read_dir(path) {
            Ok(rd) => rd,
            Err(e) => return make_err(ctx, &format!("directory-files: {}", e)),
        };
        // collect filenames, then build list in reverse (cons prepends)
        let mut names: Vec<String> = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) => {
                    if let Some(name) = e.file_name().to_str() {
                        names.push(name.to_string());
                    }
                }
                Err(e) => return make_err(ctx, &format!("directory-files: {}", e)),
            }
        }
        let mut result = ffi::get_null();
        for name in names.iter().rev() {
            let c_name = CString::new(name.as_str()).unwrap_or_default();
            let s = ffi::sexp_c_str(ctx, c_name.as_ptr(), name.len() as ffi::sexp_sint_t);
            let _s_root = ffi::GcRoot::new(ctx, s);
            let _tail_root = ffi::GcRoot::new(ctx, result);
            result = ffi::sexp_cons(ctx, s, result);
        }
        result
    }
}

/// `create-directory`: checks FsPolicy write access, creates directory.
unsafe extern "C" fn create_directory_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "create-directory") {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = check_write(ctx, path, "create-directory") {
            return e;
        }
        match std::fs::create_dir(path) {
            Ok(()) => ffi::get_void(),
            Err(e) => make_err(ctx, &format!("create-directory: {}", e)),
        }
    }
}

/// `delete-directory`: checks FsPolicy write access, removes empty directory.
unsafe extern "C" fn delete_directory_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "delete-directory") {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = check_write(ctx, path, "delete-directory") {
            return e;
        }
        match std::fs::remove_dir(path) {
            Ok(()) => ffi::get_void(),
            Err(e) => make_err(ctx, &format!("delete-directory: {}", e)),
        }
    }
}

/// `rename-file`: checks FsPolicy write access on both paths.
///
/// extracts two string arguments: old-path and new-path.
unsafe extern "C" fn rename_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        // extract first arg (old path)
        let old_path = match extract_string_arg(ctx, args, "rename-file") {
            Ok(s) => s,
            Err(e) => return e,
        };
        // extract second arg (new path)
        let rest = ffi::sexp_cdr(args);
        let new_path = match extract_string_arg(ctx, rest, "rename-file") {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = check_write(ctx, old_path, "rename-file") {
            return e;
        }
        if let Err(e) = check_write(ctx, new_path, "rename-file") {
            return e;
        }
        match std::fs::rename(old_path, new_path) {
            Ok(()) => ffi::get_void(),
            Err(e) => make_err(ctx, &format!("rename-file: {}", e)),
        }
    }
}

/// `current-directory`: returns current working directory as string.
///
/// no FsPolicy check — knowing the cwd is not a filesystem read operation.
unsafe extern "C" fn current_directory_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        match std::env::current_dir() {
            Ok(p) => {
                let s = p.to_string_lossy();
                let c_s = CString::new(s.as_ref()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_s.as_ptr(), s.len() as ffi::sexp_sint_t)
            }
            Err(e) => make_err(ctx, &format!("current-directory: {}", e)),
        }
    }
}

// --- registration ---

/// register all `(tein filesystem)` native trampolines into the top-level env.
///
/// must be called during `Context::build()` after context creation but before
/// any `(import (tein filesystem))`. the trampolines overwrite the scheme-level
/// stubs in `filesystem.scm` via eval.c patch H (native proc import fallback).
pub(crate) fn register_filesystem_trampolines(ctx: &crate::Context) -> crate::Result<()> {
    ctx.define_fn_variadic("file-exists?", file_exists_trampoline)?;
    ctx.define_fn_variadic("delete-file", delete_file_trampoline)?;
    ctx.define_fn_variadic("file-directory?", file_directory_trampoline)?;
    ctx.define_fn_variadic("file-regular?", file_regular_trampoline)?;
    ctx.define_fn_variadic("file-link?", file_link_trampoline)?;
    ctx.define_fn_variadic("file-size", file_size_trampoline)?;
    ctx.define_fn_variadic("directory-files", directory_files_trampoline)?;
    ctx.define_fn_variadic("create-directory", create_directory_trampoline)?;
    ctx.define_fn_variadic("delete-directory", delete_directory_trampoline)?;
    ctx.define_fn_variadic("rename-file", rename_file_trampoline)?;
    ctx.define_fn_variadic("current-directory", current_directory_trampoline)?;
    Ok(())
}
