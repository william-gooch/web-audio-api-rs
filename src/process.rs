//! Audio processing code that runs on the audio rendering thread

use std::collections::HashMap;

use crate::buffer::AudioBuffer;
use crate::buffer2::AudioBuffer as AudioBuffer2;
use crate::context::AudioParamId;
use crate::graph::{Node, NodeIndex};
use crate::SampleRate;

/// Interface for audio processing code that runs on the audio rendering thread.
///
/// Note that the AudioProcessor is typically constructed together with an `AudioNode`
/// (the user facing object that lives in the control thread). See `[crate::context::BaseAudioContext::register]`.
pub trait AudioProcessor: Send {
    /// Render an audio quantum for the given timestamp and input buffers
    fn process(
        &mut self,
        inputs: &[&AudioBuffer],
        outputs: &mut [AudioBuffer],
        params: AudioParamValues,
        timestamp: f64,
        sample_rate: SampleRate,
    );

    /// Indicate if this Node currently has tail-time, meaning it can provide output when no inputs are supplied.
    ///
    /// Tail time is `true` for source nodes (as long as they are still generating audio).
    ///
    /// Tail time is `false` for nodes that only transform their inputs.
    fn tail_time(&self) -> bool;
}

pub trait AudioProcessor2: Send {
    fn process<'a>(
        &mut self,
        inputs: &[&crate::buffer2::AudioBuffer<'a>],
        outputs: &mut [crate::buffer2::AudioBuffer<'a>],
        params: AudioParamValues,
        timestamp: f64,
        sample_rate: SampleRate,
    ) {
        todo!()
    }
    fn tail_time(&self) -> bool {
        todo!()
    }
}

impl<T> AudioProcessor2 for T where T: Send {}

/// Accessor for current [`crate::param::AudioParam`] values
///
/// Provided to implementations of [`AudioProcessor`] in the render thread
pub struct AudioParamValues<'a> {
    nodes: &'a HashMap<NodeIndex, Node<'a>>,
}

impl<'a> AudioParamValues<'a> {
    pub(crate) fn from(nodes: &'a HashMap<NodeIndex, Node<'a>>) -> Self {
        Self { nodes }
    }

    pub(crate) fn get_raw(&self, index: &AudioParamId) -> &AudioBuffer2 {
        self.nodes.get(&index.into()).unwrap().get_buffer()
    }

    /// Get the computed values for the given [`crate::param::AudioParam`]
    ///
    /// For both A & K-rate params, it will provide a slice of length [`crate::BUFFER_SIZE`]
    pub fn get(&self, index: &AudioParamId) -> &[f32] {
        &self.get_raw(index).channel_data(0)[..]
    }
}
