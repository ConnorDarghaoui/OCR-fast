/// Engine OCR real respaldado por ONNX Runtime.
pub mod onnx;
/// Engine OCR stub para UI, tests y degradación controlada.
pub mod stub;

/// Reexporta el engine stub para integración directa.
pub use stub::StubOcrEngine;
