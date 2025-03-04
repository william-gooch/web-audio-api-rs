use crate::context::{AudioContextRegistration, BaseAudioContext};
use crate::render::{AudioParamValues, AudioProcessor, AudioRenderQuantum, RenderScope};

use super::{
    AudioNode, ChannelConfig, ChannelConfigOptions, ChannelCountMode, ChannelInterpretation,
};

/// Representing the final audio destination and is what the user will ultimately hear.
pub struct AudioDestinationNode {
    registration: AudioContextRegistration,
    channel_config: ChannelConfig,
}

struct DestinationRenderer {}

impl AudioProcessor for DestinationRenderer {
    fn process(
        &mut self,
        inputs: &[AudioRenderQuantum],
        outputs: &mut [AudioRenderQuantum],
        _params: AudioParamValues,
        _scope: &RenderScope,
    ) -> bool {
        // single input/output node
        let input = &inputs[0];
        let output = &mut outputs[0];

        // just move input to output
        *output = input.clone();

        true
    }
}

impl AudioNode for AudioDestinationNode {
    fn registration(&self) -> &AudioContextRegistration {
        &self.registration
    }

    fn channel_config(&self) -> &ChannelConfig {
        &self.channel_config
    }

    fn number_of_inputs(&self) -> usize {
        1
    }
    fn number_of_outputs(&self) -> usize {
        1
    }

    fn set_channel_count(&self, v: usize) {
        if self.registration.context().offline() && v != self.max_channels_count() {
            panic!("NotSupportedError: not allowed to change OfflineAudioContext destination channel count");
        }
        if v > self.max_channels_count() {
            panic!(
                "IndexSizeError: channel count cannot be greater than maxChannelCount ({})",
                self.max_channels_count()
            );
        }
        self.channel_config.set_count(v);
    }
    fn set_channel_count_mode(&self, _v: ChannelCountMode) {
        panic!("InvalidStateError: AudioDestinationNode has channel count mode constraints");
    }
}

impl AudioDestinationNode {
    pub(crate) fn new<C: BaseAudioContext>(context: &C, channel_count: usize) -> Self {
        context.register(move |registration| {
            let channel_config = ChannelConfigOptions {
                count: channel_count,
                mode: ChannelCountMode::Explicit,
                interpretation: ChannelInterpretation::Speakers,
            }
            .into();
            let node = Self {
                registration,
                channel_config,
            };
            let proc = DestinationRenderer {};

            (node, Box::new(proc))
        })
    }

    pub(crate) fn into_channel_config(self) -> ChannelConfig {
        self.channel_config
    }

    pub(crate) fn from_raw_parts(
        registration: AudioContextRegistration,
        channel_config: ChannelConfig,
    ) -> Self {
        Self {
            registration,
            channel_config,
        }
    }
    /// The maximum number of channels that the channelCount attribute can be set to (the max
    /// number of channels that the hardware is capable of supporting).
    pub fn max_channels_count(&self) -> usize {
        self.registration.context().base().max_channel_count()
    }
}
