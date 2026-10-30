@echo off
rem ============================================================
rem  build_all.bat -- full build of every crate in the tree.
rem
rem  What this builds, in order:
rem   1. the root workspace     (rfs-client + rhi, examples, test targets)
rem   2. server_src workspace   (core, damage, net, server, ship, sim, stress)
rem   3. Tool workspace         (harness + client_tests + wireaudit)
rem
rem  What this deliberately does NOT do:
rem   - it runs no tests: the client_tests suite renders on the GPU, and
rem     one of its tests (`compile_shaders -- --ignored`) REGENERATES the
rem     committed .spv assets -- a build must never touch assets;
rem   - it runs no clean and deletes nothing: incremental builds only,
rem     into the standard `target` directories;
rem   - it sets nothing permanently: no setx, every variable lives and
rem     dies with this script's own setlocal;
rem   - it never renames, moves or commits anything.
rem
rem  Every step checks the exit code and stops on the first failure, so
rem  "DONE" at the end really means "everything compiled, 0 errors".
rem
rem  CARGO_TARGET_DIR: left as inherited from the environment on purpose.
rem  If the shell already points it somewhere, every step builds there
rem  (consistent across all three workspaces); if it is unset, the
rem  standard per-workspace `target` directories are used.
rem ============================================================
setlocal
cd /d "%~dp0"

echo [1/3] root workspace: rfs-client + rhi + examples + test targets
cargo build --workspace --all-targets
if errorlevel 1 (
    echo FAILED at step 1: the root workspace did not build.
    goto failed
)

echo [2/3] server_src workspace: core, damage, net, server, ship, sim, stress
pushd server_src
cargo build --workspace --all-targets
if errorlevel 1 (
    popd
    echo FAILED at step 2: the server workspace did not build.
    goto failed
)
popd

echo [3/3] Tool workspace: harness + client_tests + wireaudit
pushd Tool
cargo build --workspace --all-targets
if errorlevel 1 (
    popd
    echo FAILED at step 3: the Tool workspace did not build.
    goto failed
)
popd

echo DONE: every crate built, 0 errors.
exit /b 0

:failed
echo The build stopped at the step printed above. Fix it and run again.
exit /b 1
