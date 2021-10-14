mod codegen_x64;
mod emitter_x64;
mod machine;
#[cfg(target_arch = "aarch64")]
mod translator_aarch64;

pub use codegen_x64::X64FunctionCode as FunctionCodeGenerator;
pub use codegen_x64::X64ModuleCodeGenerator as ModuleCodeGenerator;
