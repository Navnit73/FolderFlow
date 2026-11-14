use std::io::ErrorKind;
use std::sync::Arc;

use super::fault::{FaultKind, FaultyFs, FsOpKind};
use super::{Blake3Hash, Fs, SafeFs};
use crate::engine::fs_view::EntryKind;
use crate::test_support::{mkdir, symlink, tempdir, write_file};

fn test_fs(root: &std::path::Path) -> SafeFs {
    SafeFs::with_trash_dir(root.join(".Trash"))
}

#[test]
fn r_sfs_01_rename_excl_moves_file_and_refuses_existing_destination() {
    let (_d, root) = tempdir();
    let fs = test_fs(&root);
    let src = root.join("invoice.pdf");
    let dst = root.join("moved.pdf");
    write_file(&src, b"original-invoice");

    let pre = fs.lstat(&src).expect("lstat src");
    fs.rename_excl(&src, &dst)
        .expect("rename_excl to unoccupied dst");
    assert!(!fs.exists_no_follow(&src), "source should be moved");
    let post = fs.lstat(&dst).expect("lstat dst");
    assert_eq!(pre.ino, post.ino, "same-volume rename preserves inode");

    // Now try rename_excl onto an occupied destination: must fail with AlreadyExists
    // and leave both files untouched (SAFETY_MODEL S1).
    let src2 = root.join("second.pdf");
    write_file(&src2, b"second-content");
    let err = fs
        .rename_excl(&src2, &dst)
        .expect_err("must refuse existing destination");
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    assert_eq!(
        std::fs::read(&dst).expect("read dst"),
        b"original-invoice",
        "existing destination must never be overwritten"
    );
    assert_eq!(
        std::fs::read(&src2).expect("read src2"),
        b"second-content",
        "source must remain intact on failed rename_excl"
    );
}

#[test]
fn r_sfs_02_rename_excl_refuses_dangling_symlink_at_destination() {
    let (_d, root) = tempdir();
    let fs = test_fs(&root);
    let src = root.join("source.txt");
    let dangling = root.join("dangling_link.txt");
    write_file(&src, b"precious");
    symlink(root.join("nonexistent_target"), &dangling);

    assert!(
        fs.exists_no_follow(&dangling),
        "exists_no_follow must detect a dangling symlink"
    );
    let err = fs
        .rename_excl(&src, &dangling)
        .expect_err("must refuse dangling symlink at destination");
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    assert!(fs.exists_no_follow(&src));
    assert_eq!(
        fs.lstat(&dangling).expect("lstat link").kind,
        EntryKind::Symlink
    );
}

#[test]
fn r_sfs_03_copy_excl_streams_hashes_and_refuses_existing_destination() {
    let (_d, root) = tempdir();
    let fs = test_fs(&root);
    let src = root.join("large.bin");
    let dst = root.join("copied.bin");
    // > 64 KiB payload to exercise multi-chunk 64 KiB streaming + BLAKE3 hashing.
    let payload: Vec<u8> = (0..150_000usize).map(|i| (i % 251) as u8).collect();
    write_file(&src, &payload);

    let out = fs.copy_excl(&src, &dst).expect("copy_excl");
    assert_eq!(out.bytes_copied, payload.len() as u64);
    let expected_hash = Blake3Hash(*blake3::hash(&payload).as_bytes());
    assert_eq!(out.blake3, expected_hash);
    assert_eq!(std::fs::read(&dst).expect("read dst"), payload);

    // Calling copy_excl onto an existing destination must fail with AlreadyExists (O_CREAT|O_EXCL).
    let src2 = root.join("other.bin");
    write_file(&src2, b"different");
    let err = fs
        .copy_excl(&src2, &dst)
        .expect_err("copy_excl must refuse existing destination");
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    assert_eq!(
        std::fs::read(&dst).expect("read dst"),
        payload,
        "existing destination must remain intact"
    );
}

#[test]
fn r_sfs_04_copy_excl_and_rename_excl_fsync_file_and_parent_dir() {
    let (_d, root) = tempdir();
    let fs = test_fs(&root);
    let src = root.join("a.txt");
    let staged = root.join("a.partial");
    let final_dst = root.join("b.txt");
    write_file(&src, b"hello world");

    assert_eq!(fs.fsync_count(), 0);
    fs.copy_excl(&src, &staged).expect("copy_excl");
    // copy_excl must fsync the written file AND the parent directory (>= 2 fsyncs).
    let after_copy = fs.fsync_count();
    assert!(
        after_copy >= 2,
        "expected >= 2 fsyncs after copy_excl (file + parent dir), got {after_copy}"
    );

    fs.rename_excl(&staged, &final_dst).expect("rename_excl");
    let after_rename = fs.fsync_count();
    assert!(
        after_rename > after_copy,
        "rename_excl must fsync destination parent directory"
    );
}

#[test]
fn r_sfs_05_move_to_trash_relocates_without_deleting_and_handles_collisions() {
    let (_d, root) = tempdir();
    let fs = test_fs(&root);
    let f1 = root.join("dir1/dup.txt");
    let f2 = root.join("dir2/dup.txt");
    write_file(&f1, b"first-version");
    write_file(&f2, b"second-version");

    let t1 = fs.move_to_trash(&f1).expect("trash first file");
    let t2 = fs
        .move_to_trash(&f2)
        .expect("trash second file with same name");

    assert!(!fs.exists_no_follow(&f1));
    assert!(!fs.exists_no_follow(&f2));
    assert_ne!(t1, t2, "colliding names in Trash must get distinct paths");
    assert_eq!(std::fs::read(&t1).expect("read t1"), b"first-version");
    assert_eq!(std::fs::read(&t2).expect("read t2"), b"second-version");
}

#[test]
fn r_sfs_06_rename_excl_surfaces_exdev_for_cross_volume_fallback() {
    let (_d, root) = tempdir();
    let vol_a = root.join("vol_a");
    let vol_b = root.join("vol_b");
    mkdir(&vol_a);
    mkdir(&vol_b);
    let inner = Arc::new(test_fs(&root));
    let faulty = FaultyFs::new(inner);
    faulty.simulate_cross_volume(vol_a.clone(), vol_b.clone());

    let src = vol_a.join("movie.mp4");
    let dst = vol_b.join("movie.mp4");
    write_file(&src, b"video-bytes");

    let err = faulty
        .rename_excl(&src, &dst)
        .expect_err("cross-volume rename must fail with EXDEV");
    assert_eq!(err.raw_os_error(), Some(libc::EXDEV));
    assert!(faulty.exists_no_follow(&src), "source untouched on EXDEV");
}

#[test]
fn r_sfs_07_symlink_in_destination_parent_chain_is_refused() {
    let (_d, root) = tempdir();
    let (_o, outside) = tempdir();
    let fs = test_fs(&root);

    let real_sub = root.join("safe/sub");
    mkdir(&real_sub);
    assert!(fs
        .ensure_no_symlink_ancestors(&real_sub.join("file.txt"), &root)
        .is_ok());

    let sym_dir = root.join("safe/escaped");
    symlink(&outside, &sym_dir);
    let bad_target = sym_dir.join("nested/file.txt");
    let err = fs
        .ensure_no_symlink_ancestors(&bad_target, &root)
        .expect_err("symlink in destination parent chain must be refused");
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    // mkdir_p through a symlinked component must also be refused.
    assert!(fs.mkdir_p(&sym_dir.join("nested")).is_err());
}

#[test]
fn r_sfs_08_remove_verified_source_requires_verified_token_and_matching_identity() {
    let (_d, root) = tempdir();
    let fs = test_fs(&root);
    let src = root.join("source.dat");
    let dst = root.join("dest.dat");
    write_file(&src, b"immutable-payload");

    let pre = fs.lstat(&src).expect("lstat src");
    let copied = fs.copy_excl(&src, &dst).expect("copy_excl");

    // 1. If destination was corrupted before verify_copied, verify_copied must fail.
    write_file(&dst, b"corrupted-payload");
    assert!(
        fs.verify_copied(&src, &pre, &dst, &copied).is_err(),
        "verify_copied must reject corrupted destination"
    );

    // Restore valid destination and verify token acquisition succeeds.
    let dst2 = root.join("dest_ok.dat");
    let copied2 = fs.copy_excl(&src, &dst2).expect("copy_excl 2");
    let token = fs
        .verify_copied(&src, &pre, &dst2, &copied2)
        .expect("verify_copied should succeed");

    // 2. If source changes AFTER token is issued, remove_verified_source must refuse to unlink!
    write_file(&src, b"modified-after-verify-with-different-len");
    let err = fs
        .remove_verified_source(&src, token)
        .expect_err("must refuse to remove source modified after verification");
    assert_eq!(err.kind(), ErrorKind::InvalidData);
    assert!(
        fs.exists_no_follow(&src),
        "modified source must not be deleted"
    );

    // 3. With unmodified source, verify_copied + remove_verified_source unlinks source cleanly.
    let pre3 = fs.lstat(&src).expect("lstat modified src");
    let dst3 = root.join("dest_final.dat");
    let copied3 = fs.copy_excl(&src, &dst3).expect("copy_excl 3");
    let token3 = fs
        .verify_copied(&src, &pre3, &dst3, &copied3)
        .expect("verify_copied 3");
    fs.remove_verified_source(&src, token3)
        .expect("remove_verified_source succeeds when source identity matches");
    assert!(!fs.exists_no_follow(&src));
    assert!(fs.exists_no_follow(&dst3));
}

#[test]
fn r_sfs_09_faulty_fs_injects_enospc_eacces_enoent_exdev_at_step_n() {
    let (_d, root) = tempdir();
    let inner = Arc::new(test_fs(&root));
    let faulty = FaultyFs::new(inner);

    // Step 1: mkdir_p succeeds. Step 2: copy_excl fails with ENOSPC. Step 3: rename_excl fails with EACCES.
    faulty.fail_at_step(2, FaultKind::Errno(libc::ENOSPC));
    faulty.fail_at_step(3, FaultKind::Errno(libc::EACCES));

    let dir = root.join("sub");
    faulty.mkdir_p(&dir).expect("step 1 mkdir_p succeeds");

    let src = root.join("src.txt");
    write_file(&src, b"data");
    let err2 = faulty
        .copy_excl(&src, &dir.join("copy.txt"))
        .expect_err("step 2 must fail with ENOSPC");
    assert_eq!(err2.raw_os_error(), Some(libc::ENOSPC));

    let err3 = faulty
        .rename_excl(&src, &dir.join("moved.txt"))
        .expect_err("step 3 must fail with EACCES");
    assert_eq!(err3.raw_os_error(), Some(libc::EACCES));

    // Also test fail_next_op with ENOENT and EXDEV.
    faulty.fail_next_op(FsOpKind::MoveToTrash, FaultKind::Errno(libc::ENOENT));
    let err4 = faulty
        .move_to_trash(&src)
        .expect_err("MoveToTrash must fail with ENOENT");
    assert_eq!(err4.raw_os_error(), Some(libc::ENOENT));

    assert_eq!(faulty.step_count(), 4);
    assert_eq!(faulty.history().len(), 4);
}

#[test]
fn r_sfs_10_faulty_fs_simulates_mid_copy_partial_write_failure() {
    let (_d, root) = tempdir();
    let inner = Arc::new(test_fs(&root));
    let faulty = FaultyFs::new(inner);

    let src = root.join("full.bin");
    let partial = root.join(".folderflow-op1.partial");
    write_file(&src, &[42u8; 4096]);

    faulty.fail_next_op(
        FsOpKind::CopyExcl,
        FaultKind::PartialCopy {
            bytes: 512,
            errno: libc::ENOSPC,
        },
    );

    let err = faulty
        .copy_excl(&src, &partial)
        .expect_err("partial copy must fail with ENOSPC");
    assert_eq!(err.raw_os_error(), Some(libc::ENOSPC));
    assert!(
        faulty.exists_no_follow(&partial),
        "partial staging file should exist on disk after mid-copy failure"
    );
    assert_eq!(
        faulty.lstat(&partial).expect("lstat partial").size,
        512,
        "partial file should contain the truncated bytes written before crash"
    );
}

#[test]
fn bug_021_mkdir_p_allows_macos_root_system_links_while_rejecting_other_symlinks() {
    let raw_tmp = tempfile::TempDir::new().expect("raw tempdir under /var/folders");
    let fs = SafeFs::production();
    let nested = raw_tmp.path().join("a/b/c");
    fs.mkdir_p(&nested)
        .expect("mkdir_p must succeed under /var/folders system link");
    assert!(nested.is_dir());

    // Any non-root symlink inside the tree must still be rejected.
    let link = raw_tmp.path().join("user_link");
    std::os::unix::fs::symlink(&nested, &link).expect("create symlink");
    let under_link = link.join("sub");
    let err = fs
        .mkdir_p(&under_link)
        .expect_err("user symlink component must still be rejected");
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
}

#[test]
fn p8_safe_fs_edge_cases_and_trait_coverage() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;

    let (_d, root) = tempdir();
    let default_fs = SafeFs::default();
    let trash_dir = root.join(".Trash");
    let fs = SafeFs::with_trash_dir(trash_dir.clone());

    // 1. CopyOutput & Blake3Hash Debug/Display helpers
    let src = root.join("sample.bin");
    write_file(&src, b"hello-safe-fs");
    let dst = root.join("copy.bin");
    let out = fs.copy_excl(&src, &dst).expect("copy_excl");
    let hex_disp = format!("{}", out.blake3);
    let dbg_disp = format!("{:?}", out.blake3);
    assert!(dbg_disp.starts_with("Blake3Hash("));
    assert_eq!(hex_disp.len(), 64);

    // 2. Non-regular file rejected by copy_excl / hash_file
    let dir_target = root.join("adir");
    mkdir(&dir_target);
    let err = fs
        .copy_excl(&dir_target, &root.join("dir_copy"))
        .expect_err("directory cannot be copy_excl'd");
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    // 3. rename_excl interior NUL validation
    let nul_path = std::path::Path::new(OsStr::from_bytes(b"/tmp/bad\0src"));
    assert_eq!(
        fs.rename_excl(nul_path, &root.join("ok.txt"))
            .expect_err("NUL src")
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        fs.rename_excl(&src, nul_path).expect_err("NUL dst").kind(),
        ErrorKind::InvalidInput
    );

    // 4. ensure_no_symlink_ancestors & mkdir_p reject file ancestors
    let err_anc = fs
        .ensure_no_symlink_ancestors(&src.join("child.txt"), &root)
        .expect_err("file ancestor must be rejected");
    assert_eq!(err_anc.kind(), ErrorKind::InvalidInput);

    let err_mk = fs
        .mkdir_p(&src.join("sub"))
        .expect_err("mkdir_p under file component must fail");
    assert_eq!(err_mk.kind(), ErrorKind::AlreadyExists);

    // 5. remove_verified_source rejects wrong path token
    let pre = fs.lstat(&src).expect("lstat src");
    let token = fs
        .verify_copied(&src, &pre, &dst, &out)
        .expect("verify_copied");
    assert_eq!(token.blake3(), out.blake3);
    let wrong_path_err = fs
        .remove_verified_source(&root.join("other.txt"), token)
        .expect_err("token bound to original source_path");
    assert_eq!(wrong_path_err.kind(), ErrorKind::InvalidInput);

    // 6. stat_fs_free_bytes walks up nonexistent children
    let deep_nonexistent = root.join("missing/nested/deep/file.bin");
    let free = fs
        .stat_fs_free_bytes(&deep_nonexistent)
        .expect("stat_fs_free_bytes on missing descendant");
    assert!(free > 0);

    // 7. FaultyFs forwarding & fsync_dir fault coverage
    let faulty = FaultyFs::new(Arc::new(fs.clone()));
    assert_eq!(faulty.hash_file(&src).expect("hash_file").1, out.blake3);
    assert!(faulty
        .rmdir_if_empty(&dir_target)
        .expect("empty dir removed"));
    assert!(!faulty
        .rmdir_if_empty(&dir_target)
        .expect("already absent returns false"));
    assert!(faulty.fsync_count() >= 1);
    faulty.fail_next_op(FsOpKind::FsyncDir, FaultKind::Errno(libc::EIO));
    let fsync_err = faulty
        .fsync_dir(&root)
        .expect_err("faulty fsync_dir should fail");
    assert_eq!(fsync_err.raw_os_error(), Some(libc::EIO));

    // 8. Cross-volume trash fallback (move_into_trash_dir via FaultyFs EXDEV) & extensionless naming
    let xv_src_dir = root.join("vol_a");
    mkdir(&xv_src_dir);
    mkdir(&trash_dir);
    let no_ext1 = xv_src_dir.join("README");
    let no_ext2 = xv_src_dir.join("README");
    write_file(&no_ext1, b"first-readme");
    faulty.simulate_cross_volume(xv_src_dir.clone(), trash_dir.clone());
    let t1 = super::trash::move_into_trash_dir(&faulty, &no_ext1, &trash_dir)
        .expect("EXDEV move to trash #1");
    assert!(t1.ends_with("README"));
    assert!(!fs.exists_no_follow(&no_ext1));

    write_file(&no_ext2, b"second-readme");
    let t2 = super::trash::move_into_trash_dir(&faulty, &no_ext2, &trash_dir)
        .expect("EXDEV collision suffix move to trash #2");
    assert!(t2.ends_with("README (1)"), "got {}", t2.display());
    assert_eq!(std::fs::read(&t2).expect("read t2"), b"second-readme");

    // EXDEV copy_excl AlreadyExists race + non-AlreadyExists error + trailing dot name + rmdir_if_empty on file
    let trailing_dot = xv_src_dir.join("trailing.");
    write_file(&trailing_dot, b"dot-bytes");
    faulty.fail_next_op(FsOpKind::CopyExcl, FaultKind::Errno(libc::EEXIST));
    let t_dot = super::trash::move_into_trash_dir(&faulty, &trailing_dot, &trash_dir)
        .expect("EXDEV trailing dot retries AlreadyExists cleanly");
    assert!(t_dot.ends_with("trailing. (1)"), "got {}", t_dot.display());

    let exdev_fail = xv_src_dir.join("exdev_err.txt");
    write_file(&exdev_fail, b"err");
    faulty.fail_next_op(FsOpKind::CopyExcl, FaultKind::Errno(libc::ENOSPC));
    assert_eq!(
        super::trash::move_into_trash_dir(&faulty, &exdev_fail, &trash_dir)
            .expect_err("copy_excl ENOSPC propagates")
            .raw_os_error(),
        Some(libc::ENOSPC)
    );
    faulty.fail_next_op(FsOpKind::RenameExcl, FaultKind::Errno(libc::EIO));
    assert_eq!(
        super::trash::move_into_trash_dir(&faulty, &exdev_fail, &trash_dir)
            .expect_err("rename_excl EIO propagates")
            .raw_os_error(),
        Some(libc::EIO)
    );
    assert!(
        fs.rmdir_if_empty(&dst).is_err(),
        "rmdir on regular file fails ENOTDIR"
    );

    // 9. Production NSFileManager Trash paths (NotFound, UTF-8 validation, PermissionDenied, real Trash + cleanup)
    let missing = root.join("does-not-exist-in-prod-trash.txt");
    let prod_missing = default_fs
        .move_to_trash(&missing)
        .expect_err("missing file fails NotFound");
    assert_eq!(prod_missing.kind(), ErrorKind::NotFound);

    let non_utf8 = std::path::Path::new(OsStr::from_bytes(b"/tmp/invalid_\xff_name"));
    let non_utf8_err = default_fs
        .move_to_trash(non_utf8)
        .expect_err("non-UTF-8 rejected");
    assert_eq!(non_utf8_err.kind(), ErrorKind::InvalidInput);

    // PermissionDenied via read-only directory on NSFileManager
    let ro_dir = root.join("ro_dir");
    mkdir(&ro_dir);
    let ro_file = ro_dir.join("locked.txt");
    write_file(&ro_file, b"locked");
    #[allow(clippy::disallowed_methods)]
    {
        std::fs::set_permissions(&ro_dir, std::fs::Permissions::from_mode(0o555))
            .expect("chmod 555");
    }
    let ro_err = default_fs
        .move_to_trash(&ro_file)
        .expect_err("trashing inside read-only dir fails with PermissionDenied");
    assert_eq!(ro_err.kind(), ErrorKind::PermissionDenied);
    assert_eq!(
        fs.mkdir_p(&ro_dir.join("blocked_subdir"))
            .expect_err("mkdir_p inside read-only parent fails")
            .kind(),
        ErrorKind::PermissionDenied
    );
    #[allow(clippy::disallowed_methods)]
    {
        std::fs::set_permissions(&ro_dir, std::fs::Permissions::from_mode(0o755))
            .expect("restore chmod");
    }

    assert!(
        fs.stat_fs_free_bytes(std::path::Path::new("nonexistent_rel_entry"))
            .expect("relative fallback to /")
            > 0
    );

    // Exercise happy path of NSFileManager trash and immediately remove our uniquely named temp item from ~/.Trash.
    let prod_tmp = root.join("folderflow_p8_trash_probe.txt");
    write_file(&prod_tmp, b"trash-probe");
    if let Ok(trashed_real) = default_fs.move_to_trash(&prod_tmp) {
        assert!(!fs.exists_no_follow(&prod_tmp));
        if let Some(name) = trashed_real.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("folderflow_p8_trash_probe") {
                #[allow(clippy::disallowed_methods)]
                let _ = std::fs::remove_file(&trashed_real);
            }
        }
    }
}
