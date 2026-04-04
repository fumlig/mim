use std::path::Path;

use ndarray::{Array1, Array2, ArrayD};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Value;

const CHUNK_SIZE: usize = 512;
const CONTEXT_SIZE: usize = 64;
const SAMPLE_RATE: i64 = 16_000;

/// Silero VAD v5 wrapper with proper context handling.
pub struct Silero {
    session: Session,
    sample_rate: Array1<i64>,
    state: ArrayD<f32>,
    context: Array1<f32>,
}

impl Silero {
    /// Load a Silero VAD ONNX model from `model_path`.
    ///
    /// Use `scripts/download-silero-vad.sh` to fetch the model into
    /// `models/silero_vad.onnx` at the repo root.
    pub fn new(model_path: &Path) -> Result<Self, ort::Error> {
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(1)?
            .with_inter_threads(1)?
            .commit_from_file(model_path)?;

        Ok(Self {
            session,
            sample_rate: Array1::from_vec(vec![SAMPLE_RATE]),
            state: ArrayD::zeros(vec![2, 1, 128]),
            context: Array1::zeros(CONTEXT_SIZE),
        })
    }

    /// Predict speech probability for a 512-sample i16 chunk.
    pub fn predict(&mut self, audio: &[i16]) -> Result<f32, ort::Error> {
        let data: Vec<f32> = audio
            .iter()
            .take(CHUNK_SIZE)
            .map(|&s| s as f32 / i16::MAX as f32)
            .collect();

        // Prepend context from previous chunk.
        let mut input = Vec::with_capacity(CONTEXT_SIZE + data.len());
        input.extend_from_slice(self.context.as_slice().unwrap());
        input.extend_from_slice(&data);

        let frame = Array2::from_shape_vec([1, input.len()], input).unwrap();
        let state = std::mem::replace(&mut self.state, ArrayD::zeros(vec![2, 1, 128]));

        let frame_val = Value::from_array(frame)?;
        let state_val = Value::from_array(state)?;
        let sr_val = Value::from_array(self.sample_rate.clone())?;

        let outputs = self.session.run([
            (&frame_val).into(),
            (&state_val).into(),
            (&sr_val).into(),
        ])?;

        // Update RNN state.
        let (shape, state_data) = outputs["stateN"].try_extract_tensor::<f32>()?;
        let shape: Vec<usize> = shape.as_ref().iter().map(|&d| d as usize).collect();
        self.state = ArrayD::from_shape_vec(shape.as_slice(), state_data.to_vec()).unwrap();

        // Update context with the last 64 samples.
        if data.len() >= CONTEXT_SIZE {
            self.context =
                Array1::from_vec(data[data.len() - CONTEXT_SIZE..].to_vec());
        }

        let prob = *outputs["output"]
            .try_extract_tensor::<f32>()?
            .1
            .first()
            .unwrap();
        Ok(prob)
    }

    pub fn reset(&mut self) {
        self.state = ArrayD::zeros(vec![2, 1, 128]);
        self.context = Array1::zeros(CONTEXT_SIZE);
    }
}
