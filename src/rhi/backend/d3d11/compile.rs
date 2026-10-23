//! Runtime HLSL compilation for the D3D11 shader path.
//!
//! D3D11 binds finished shader containers; it has no compiler of its own. The
//! backend therefore accepts `ShaderFormat::Dxil` bytes and leaves the
//! container production to a helper everyone can share. `compile_hlsl` runs
//! `D3DCompile` (`d3dcompiler_47.dll`, ship-shipped in every Windows since XP)
//! and produces an SM 5.0 DXBC container that the same path binds without
//! further translation.

use crate::error::RhiError;

use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;

/// Compiles HLSL source text into a shader container.
///
/// `entry_point` is the stage function name, `target` the D3D compiler target
/// such as `"vs_5_0"` or `"ps_5_0"`. The returned bytes read as a finished
/// DXBC container and are fed to the backend as `ShaderFormat::Dxil`.
pub fn compile_hlsl(source: &[u8], entry_point: &str, target: &str) -> crate::RhiResult<Vec<u8>> {
    if source.is_empty() {
        return Err(RhiError::ShaderCompilationError(
            "the HLSL source text is empty".into(),
        ));
    }
    if !entry_point.is_ascii() || !target.is_ascii() {
        return Err(RhiError::ShaderCompilationError(
            "the entry point and target must be ASCII".into(),
        ));
    }
    let mut blob: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    let entry = std::ffi::CString::new(entry_point)
        .map_err(|e| RhiError::ShaderCompilationError(format!("entry point: {e}")))?;
    let stage_target = std::ffi::CString::new(target)
        .map_err(|e| RhiError::ShaderCompilationError(format!("target: {e}")))?;
    unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR::from_raw(entry.as_ptr().cast()),
            PCSTR::from_raw(stage_target.as_ptr().cast()),
            0,
            0,
            &mut blob,
            Some(&mut errors),
        )
        .map_err(|e| {
            let detail = error_text(&errors).unwrap_or_else(|| e.to_string());
            RhiError::ShaderCompilationError(format!("D3DCompile: {detail}"))
        })?;
    }
    let blob = blob.ok_or_else(|| {
        RhiError::ShaderCompilationError("D3DCompile returned no code blob".into())
    })?;
    let (pointer, size) = unsafe { (blob.GetBufferPointer(), blob.GetBufferSize()) };
    if pointer.is_null() || size == 0 {
        return Err(RhiError::ShaderCompilationError(
            "the compiler produced an empty container".into(),
        ));
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer as *const u8, size as usize) }.to_vec())
}

fn error_text(blob: &Option<ID3DBlob>) -> Option<String> {
    let blob = blob.as_ref()?;
    let size = unsafe { blob.GetBufferSize() };
    if size == 0 {
        return None;
    }
    let pointer = unsafe { blob.GetBufferPointer() } as *const u8;
    let slice = unsafe { std::slice::from_raw_parts(pointer, size as usize) };
    let end = slice.iter().position(|b| *b == 0).unwrap_or(slice.len());
    Some(String::from_utf8_lossy(&slice[..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_happy_source_becomes_a_container() {
        let code = compile_hlsl(
            b"struct VsOut { float4 pos : SV_POSITION; };\
              VsOut main(float4 pos : POSITION) { VsOut o; o.pos = pos; return o; }",
            "main",
            "vs_5_0",
        )
        .expect("the compile must succeed");
        assert!(code.len() > 16);
        assert_eq!(&code[..4], b"DXBC", "the container must start with the DXBC magic");
    }

    #[test]
    fn a_broken_source_names_the_error_line() {
        let error = compile_hlsl(b"VsOut main() { return 1; }", "main", "vs_5_0")
            .expect_err("a compile failure has to be an error");
        assert!(
            error.to_string().to_lowercase().contains("error"),
            "the refusal should carry the HLSL error line, got: {error}"
        );
    }

    #[test]
    fn an_empty_source_is_refused_before_the_compiler_awakens() {
        assert!(compile_hlsl(&[], "main", "vs_5_0").is_err());
    }
}
