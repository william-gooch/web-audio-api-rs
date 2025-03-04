use crossbeam_channel::{self, Receiver, Sender};
use std::fmt::Debug;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::context::{AudioContextRegistration, AudioParamId, BaseAudioContext};
use crate::control::Scheduler;
use crate::param::{AudioParam, AudioParamDescriptor, AutomationRate};
use crate::periodic_wave::PeriodicWave;
use crate::render::{AudioParamValues, AudioProcessor, AudioRenderQuantum, RenderScope};
use crate::RENDER_QUANTUM_SIZE;

use super::{
    AudioNode, AudioScheduledSourceNode, ChannelConfig, ChannelConfigOptions, SINETABLE,
    TABLE_LENGTH_USIZE,
};

/// Options for constructing an [`OscillatorNode`]
// dictionary OscillatorOptions : AudioNodeOptions {
//   OscillatorType type = "sine";
//   float frequency = 440;
//   float detune = 0;
//   PeriodicWave periodicWave;
// };
#[derive(Clone, Debug)]
pub struct OscillatorOptions {
    /// The shape of the periodic waveform
    pub type_: OscillatorType,
    /// The frequency of the fundamental frequency.
    pub frequency: f32,
    /// A detuning value (in cents) which will offset the frequency by the given amount.
    pub detune: f32,
    /// Optionnal custom waveform, if specified (set `type` to "custom")
    pub periodic_wave: Option<PeriodicWave>,
    /// channel config options
    pub channel_config: ChannelConfigOptions,
}

impl Default for OscillatorOptions {
    fn default() -> Self {
        Self {
            type_: OscillatorType::default(),
            frequency: 440.,
            detune: 0.,
            periodic_wave: None,
            channel_config: ChannelConfigOptions::default(),
        }
    }
}

/// Type of the waveform rendered by an `OscillatorNode`
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum OscillatorType {
    /// Sine wave
    Sine,
    /// Square wave
    Square,
    /// Sawtooth wave
    Sawtooth,
    /// Triangle wave
    Triangle,
    /// type used when periodic_wave is specified
    Custom,
}

impl Default for OscillatorType {
    fn default() -> Self {
        Self::Sine
    }
}

impl From<u32> for OscillatorType {
    fn from(i: u32) -> Self {
        match i {
            0 => OscillatorType::Sine,
            1 => OscillatorType::Square,
            2 => OscillatorType::Sawtooth,
            3 => OscillatorType::Triangle,
            4 => OscillatorType::Custom,
            _ => unreachable!(),
        }
    }
}

/// `OscillatorNode` represents an audio source generating a periodic waveform.
/// It can generate a few common waveforms (i.e. sine, square, sawtooth, triangle),
/// or can be set to an arbitrary periodic waveform using a [`PeriodicWave`] object.
///
/// - MDN documentation: <https://developer.mozilla.org/en-US/docs/Web/API/OscillatorNode>
/// - specification: <https://webaudio.github.io/web-audio-api/#OscillatorNode>
/// - see also: [`BaseAudioContext::create_oscillator`](crate::context::BaseAudioContext::create_oscillator)
/// - see also: [`PeriodicWave`](crate::periodic_wave::PeriodicWave)
///
/// # Usage
///
/// ```no_run
/// use web_audio_api::context::{BaseAudioContext, AudioContext};
/// use web_audio_api::node::{AudioNode, AudioScheduledSourceNode};
///
/// let context = AudioContext::default();
///
/// let osc = context.create_oscillator();
/// osc.frequency().set_value(200.);
/// osc.connect(&context.destination());
/// osc.start();
/// ```
///
/// # Examples
///
/// - `cargo run --release --example oscillators`
/// - `cargo run --release --example many_oscillators_with_env`
/// - `cargo run --release --example amplitude_modulation`
///
pub struct OscillatorNode {
    /// Represents the node instance and its associated audio context
    registration: AudioContextRegistration,
    /// Infos about audio node channel configuration
    channel_config: ChannelConfig,
    /// The frequency of the fundamental frequency.
    frequency: AudioParam,
    /// A detuning value (in cents) which will offset the frequency by the given amount.
    detune: AudioParam,
    /// Waveform of an oscillator
    type_: Arc<AtomicU32>,
    /// starts and stops Oscillator audio streams
    scheduler: Scheduler,
    /// channel between control and renderer parts (sender part)
    sender: Sender<PeriodicWave>,
}

impl AudioNode for OscillatorNode {
    fn registration(&self) -> &AudioContextRegistration {
        &self.registration
    }

    fn channel_config(&self) -> &ChannelConfig {
        &self.channel_config
    }

    /// `OscillatorNode` is a source node. A source node is by definition with no input
    fn number_of_inputs(&self) -> usize {
        0
    }

    /// `OscillatorNode` is a mono source node.
    fn number_of_outputs(&self) -> usize {
        1
    }
}

impl AudioScheduledSourceNode for OscillatorNode {
    fn start(&self) {
        let when = self.registration.context().current_time();
        self.start_at(when);
    }

    fn start_at(&self, when: f64) {
        self.scheduler.start_at(when);
    }

    fn stop(&self) {
        let when = self.registration.context().current_time();
        self.stop_at(when);
    }

    fn stop_at(&self, when: f64) {
        self.scheduler.stop_at(when);
    }
}

impl OscillatorNode {
    /// Returns an `OscillatorNode`
    ///
    /// # Arguments:
    ///
    /// * `context` - The `AudioContext`
    /// * `options` - The OscillatorOptions
    pub fn new<C: BaseAudioContext>(context: &C, options: OscillatorOptions) -> Self {
        context.register(move |registration| {
            let sample_rate = context.sample_rate();
            let nyquist = sample_rate / 2.;

            let OscillatorOptions {
                type_,
                frequency,
                detune,
                channel_config,
                periodic_wave,
            } = options;

            // frequency audio parameter
            let freq_param_opts = AudioParamDescriptor {
                min_value: -nyquist,
                max_value: nyquist,
                default_value: 440.,
                automation_rate: AutomationRate::A,
            };
            let (f_param, f_proc) = context.create_audio_param(freq_param_opts, &registration);
            f_param.set_value(frequency);

            // detune audio parameter
            let det_param_opts = AudioParamDescriptor {
                min_value: -153_600.,
                max_value: 153_600.,
                default_value: 0.,
                automation_rate: AutomationRate::A,
            };
            let (det_param, det_proc) = context.create_audio_param(det_param_opts, &registration);
            det_param.set_value(detune);

            let type_ = Arc::new(AtomicU32::new(type_ as u32));

            let scheduler = Scheduler::new();
            let (sender, receiver) = crossbeam_channel::bounded(1);

            let renderer = OscillatorRenderer {
                type_: type_.clone(),
                frequency: f_proc,
                detune: det_proc,
                scheduler: scheduler.clone(),
                receiver,
                phase: 0.,
                started: false,
                periodic_wave: None,
            };

            let node = Self {
                registration,
                channel_config: channel_config.into(),
                frequency: f_param,
                detune: det_param,
                type_,
                scheduler,
                sender,
            };

            // if periodic wave has been given, init it
            if let Some(p_wave) = periodic_wave {
                node.set_periodic_wave(p_wave);
            }

            (node, Box::new(renderer))
        })
    }

    /// A-rate [`AudioParam`] that defines the fondamental frequency of the
    /// oscillator, expressed in Hz
    ///
    /// The final frequency is calculated as follow: frequency * 2^(detune/1200)
    #[must_use]
    pub fn frequency(&self) -> &AudioParam {
        &self.frequency
    }

    /// A-rate [`AudioParam`] that defines a transposition according to the
    /// frequency, expressed in cents.
    ///
    /// see <https://en.wikipedia.org/wiki/Cent_(music)>
    ///
    /// The final frequency is calculated as follow: frequency * 2^(detune/1200)
    #[must_use]
    pub fn detune(&self) -> &AudioParam {
        &self.detune
    }

    /// Returns the oscillator type
    #[must_use]
    pub fn type_(&self) -> OscillatorType {
        self.type_.load(Ordering::SeqCst).into()
    }

    /// Set the oscillator type
    ///
    /// # Arguments
    ///
    /// * `type_` - oscillator type (sine, square, triangle, sawtooth)
    ///
    /// # Panics
    ///
    /// if `type_` is `OscillatorType::Custom`
    pub fn set_type(&self, type_: OscillatorType) {
        assert_ne!(
            type_,
            OscillatorType::Custom,
            "InvalidStateError: Custom type cannot be set manually"
        );

        // if periodic wave has been set specified, type_ changes are ignored
        if self.type_.load(Ordering::SeqCst) == OscillatorType::Custom as u32 {
            return;
        }

        self.type_.store(type_ as u32, Ordering::SeqCst);
    }

    /// Sets a `PeriodicWave` which describes a waveform to be used by the oscillator.
    ///
    /// Calling this sets the oscillator type to `custom`, once set to `custom`
    /// the oscillator cannot be reverted back to a standard waveform.
    pub fn set_periodic_wave(&self, periodic_wave: PeriodicWave) {
        self.type_
            .store(OscillatorType::Custom as u32, Ordering::SeqCst);

        self.sender
            .send(periodic_wave)
            .expect("Sending periodic wave to the node renderer failed");
    }
}

/// Rendering component of the oscillator node
struct OscillatorRenderer {
    /// The shape of the periodic waveform
    type_: Arc<AtomicU32>,
    /// The frequency of the fundamental frequency.
    frequency: AudioParamId,
    /// A detuning value (in cents) which will offset the frequency by the given amount.
    detune: AudioParamId,
    /// starts and stops oscillator audio streams
    scheduler: Scheduler,
    /// channel between control and renderer parts (receiver part)
    receiver: Receiver<PeriodicWave>,
    /// current phase of the oscillator
    phase: f64,
    // defines if the oscillator has started
    started: bool,
    // wavetable placeholder for custom oscillators
    periodic_wave: Option<PeriodicWave>,
}

impl AudioProcessor for OscillatorRenderer {
    fn process(
        &mut self,
        _inputs: &[AudioRenderQuantum],
        outputs: &mut [AudioRenderQuantum],
        params: AudioParamValues,
        scope: &RenderScope,
    ) -> bool {
        // single output node
        let output = &mut outputs[0];
        // 1 channel output
        output.set_number_of_channels(1);

        // check if any message was send from the control thread
        if let Ok(periodic_wave) = self.receiver.try_recv() {
            self.periodic_wave = Some(periodic_wave);
        }

        let sample_rate = scope.sample_rate as f64;
        let dt = 1. / sample_rate;
        let num_frames = RENDER_QUANTUM_SIZE;
        let next_block_time = scope.current_time + dt * num_frames as f64;

        let mut start_time = self.scheduler.get_start_at();
        let stop_time = self.scheduler.get_stop_at();

        if start_time >= next_block_time {
            output.make_silent();
            return true;
        } else if stop_time < scope.current_time {
            output.make_silent();
            return false;
        }

        let type_ = self.type_.load(Ordering::SeqCst).into();
        let channel_data = output.channel_data_mut(0);
        let frequency_values = params.get(&self.frequency);
        let detune_values = params.get(&self.detune);

        let mut current_time = scope.current_time;

        // Prevent scheduling in the past
        //
        // [spec] If 0 is passed in for this value or if the value is less than
        // currentTime, then the sound will start playing immediately
        // cf. https://webaudio.github.io/web-audio-api/#dom-audioscheduledsourcenode-start-when-when
        if !self.started && start_time < current_time {
            start_time = current_time;
        }

        for (index, output_sample) in channel_data.iter_mut().enumerate() {
            if current_time < start_time || current_time >= stop_time {
                *output_sample = 0.;
                current_time += dt;

                continue;
            }

            let frequency = frequency_values[index];
            let detune = detune_values[index];
            let computed_frequency = frequency * (detune / 1200.).exp2();

            // first sample to render
            if !self.started {
                // if start time was between last frame and current frame
                // we need to adjust the phase first
                if current_time > start_time {
                    let phase_incr = computed_frequency as f64 / sample_rate;
                    let ratio = (current_time - start_time) / dt;
                    self.phase = Self::unroll_phase(phase_incr * ratio);
                }

                self.started = true;
            }

            let phase_incr = computed_frequency as f64 / sample_rate;

            // @note: per spec all default oscillators should be rendered from a
            // wavetable, define if it worth the assle...
            // e.g. for now `generate_sine` and `generate_custom` are almost the sames
            // cf. https://webaudio.github.io/web-audio-api/#oscillator-coefficients
            *output_sample = match type_ {
                OscillatorType::Sine => self.generate_sine(),
                OscillatorType::Sawtooth => self.generate_sawtooth(phase_incr),
                OscillatorType::Square => self.generate_square(phase_incr),
                OscillatorType::Triangle => self.generate_triangle(),
                OscillatorType::Custom => self.generate_custom(),
            };

            current_time += dt;

            self.phase = Self::unroll_phase(self.phase + phase_incr);
        }

        true
    }
}

impl OscillatorRenderer {
    #[inline]
    fn generate_sine(&mut self) -> f32 {
        let position = self.phase * TABLE_LENGTH_USIZE as f64;
        let floored = position.floor();

        let prev_index = floored as usize;
        let mut next_index = prev_index + 1;
        if next_index == TABLE_LENGTH_USIZE {
            next_index = 0;
        }

        // linear interpolation into lookup table
        let k = (position - floored) as f32;
        SINETABLE[prev_index].mul_add(1. - k, SINETABLE[next_index] * k)
    }

    #[inline]
    fn generate_sawtooth(&mut self, phase_incr: f64) -> f32 {
        // offset phase to start at 0. (not -1.)
        let phase = Self::unroll_phase(self.phase + 0.5);
        let mut sample = 2.0 * phase - 1.0;
        sample -= Self::poly_blep(phase, phase_incr, cfg!(test));

        sample as f32
    }

    #[inline]
    fn generate_square(&mut self, phase_incr: f64) -> f32 {
        let mut sample = if self.phase < 0.5 { 1.0 } else { -1.0 };
        sample += Self::poly_blep(self.phase, phase_incr, cfg!(test));

        let shift_phase = Self::unroll_phase(self.phase + 0.5);
        sample -= Self::poly_blep(shift_phase, phase_incr, cfg!(test));

        sample as f32
    }

    #[inline]
    fn generate_triangle(&mut self) -> f32 {
        let mut sample = -4. * self.phase + 2.;

        if sample > 1. {
            sample = 2. - sample;
        } else if sample < -1. {
            sample = -2. - sample;
        }

        sample as f32
    }

    #[inline]
    fn generate_custom(&mut self) -> f32 {
        let periodic_wave = self.periodic_wave.as_ref().unwrap().as_slice();
        let position = self.phase * TABLE_LENGTH_USIZE as f64;
        let floored = position.floor();

        let prev_index = floored as usize;
        let mut next_index = prev_index + 1;
        if next_index == TABLE_LENGTH_USIZE {
            next_index = 0;
        }

        // linear interpolation into lookup table
        let k = (position - floored) as f32;
        periodic_wave[prev_index].mul_add(1. - k, periodic_wave[next_index] * k)
    }

    // computes the `polyBLEP` corrections to apply to aliasing signal
    // `polyBLEP` stands for `polyBandLimitedstEP`
    // This basically soften the sharp edges in square and sawtooth signals
    // to avoid infinite frequencies impulses (jumps from -1 to 1 or inverse).
    // cf. http://www.martin-finke.de/blog/articles/audio-plugins-018-polyblep-oscillator/
    //
    // @note: do not apply in tests so we can avoid relying on snapshots
    #[inline]
    fn poly_blep(mut t: f64, dt: f64, is_test: bool) -> f64 {
        if is_test {
            0.
        } else if t < dt {
            t /= dt;
            t + t - t * t - 1.0
        } else if t > 1.0 - dt {
            t = (t - 1.0) / dt;
            t.mul_add(t, t) + t + 1.0
        } else {
            0.0
        }
    }

    #[inline]
    fn unroll_phase(mut phase: f64) -> f64 {
        if phase >= 1. {
            phase -= 1.
        }

        phase
    }
}

#[cfg(test)]
mod tests {
    use float_eq::assert_float_eq;
    use std::f64::consts::PI;

    use crate::context::{BaseAudioContext, OfflineAudioContext};
    use crate::node::{AudioNode, AudioScheduledSourceNode};
    use crate::periodic_wave::{PeriodicWave, PeriodicWaveOptions};

    use super::{OscillatorNode, OscillatorOptions, OscillatorRenderer, OscillatorType};

    #[test]
    fn assert_osc_default_build_with_factory_func() {
        let default_freq = 440.;
        let default_det = 0.;
        let default_type = OscillatorType::Sine;

        let context = OfflineAudioContext::new(2, 1, 44_100.);

        let osc = context.create_oscillator();

        let freq = osc.frequency.value();
        assert_float_eq!(freq, default_freq, abs_all <= 0.);

        let det = osc.detune.value();
        assert_float_eq!(det, default_det, abs_all <= 0.);

        let type_ = osc.type_.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(type_, default_type as u32);
    }

    #[test]
    fn assert_osc_default_build() {
        let default_freq = 440.;
        let default_det = 0.;
        let default_type = OscillatorType::Sine;

        let context = OfflineAudioContext::new(2, 1, 44_100.);

        let osc = OscillatorNode::new(&context, OscillatorOptions::default());

        let freq = osc.frequency.value();
        assert_float_eq!(freq, default_freq, abs_all <= 0.);

        let det = osc.detune.value();
        assert_float_eq!(det, default_det, abs_all <= 0.);

        let type_ = osc.type_.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(type_, default_type as u32);
    }

    #[test]
    #[should_panic]
    fn set_type_to_custom_should_panic() {
        let context = OfflineAudioContext::new(2, 1, 44_100.);
        let osc = OscillatorNode::new(&context, OscillatorOptions::default());
        osc.set_type(OscillatorType::Custom);
    }

    #[test]
    fn type_is_custom_when_periodic_wave_is_some() {
        let expected_type = OscillatorType::Custom;

        let context = OfflineAudioContext::new(2, 1, 44_100.);

        let periodic_wave = PeriodicWave::new(&context, PeriodicWaveOptions::default());

        let options = OscillatorOptions {
            periodic_wave: Some(periodic_wave),
            ..OscillatorOptions::default()
        };

        let osc = OscillatorNode::new(&context, options);

        let type_ = osc.type_.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(type_, expected_type as u32);
    }

    #[test]
    fn set_type_is_ignored_when_periodic_wave_is_some() {
        let expected_type = OscillatorType::Custom;

        let context = OfflineAudioContext::new(2, 1, 44_100.);

        let periodic_wave = PeriodicWave::new(&context, PeriodicWaveOptions::default());

        let options = OscillatorOptions {
            periodic_wave: Some(periodic_wave),
            ..OscillatorOptions::default()
        };

        let osc = OscillatorNode::new(&context, options);

        osc.set_type(OscillatorType::Sine);

        let type_ = osc.type_.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(type_, expected_type as u32);
    }

    // # Test waveforms
    //
    // - for `square`, `triangle` and `sawtooth` the tests may appear a bit
    //   tautological (and they actually are) as the code from the test is the
    //   mostly as same as in the renderer, just written in a more compact way.
    //   However they should help to prevent regressions, and/or allow testing
    //   against trusted and simple implementation in case of future changes
    //   in the renderer impl, e.g. performance improvements or spec compliance:
    //   https://webaudio.github.io/web-audio-api/#oscillator-coefficients.
    //
    // - PolyBlep is not applied on `square` and `triangle` for tests, so we can
    //   compare according to a crude waveforms

    #[test]
    fn sine_raw() {
        // 1, 10, 100, 1_000, 10_000 Hz
        for i in 0..5 {
            let freq = 10_f32.powf(i as f32);
            let sample_rate = 44_100;

            let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);

            let osc = context.create_oscillator();
            osc.connect(&context.destination());
            osc.frequency().set_value(freq);
            osc.start_at(0.);

            let output = context.start_rendering_sync();
            let result = output.get_channel_data(0);

            let mut expected = Vec::<f32>::with_capacity(sample_rate);
            let mut phase: f64 = 0.;
            let phase_incr = freq as f64 / sample_rate as f64;

            for _i in 0..sample_rate {
                let sample = (phase * 2. * PI).sin();

                expected.push(sample as f32);

                phase += phase_incr;
                if phase >= 1. {
                    phase -= 1.;
                }
            }

            assert_float_eq!(result[..], expected[..], abs_all <= 1e-5);
        }
    }

    #[test]
    fn sine_raw_exact_phase() {
        // 1, 10, 100, 1_000, 10_000 Hz
        for i in 0..5 {
            let freq = 10_f32.powf(i as f32);
            let sample_rate = 44_100;

            let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);

            let osc = context.create_oscillator();
            osc.connect(&context.destination());
            osc.frequency().set_value(freq);
            osc.start_at(0.);

            let output = context.start_rendering_sync();
            let result = output.get_channel_data(0);
            let mut expected = Vec::<f32>::with_capacity(sample_rate);

            for i in 0..sample_rate {
                let phase = freq as f64 * i as f64 / sample_rate as f64;
                let sample = (phase * 2. * PI).sin();
                // phase += phase_incr;
                expected.push(sample as f32);
            }

            assert_float_eq!(result[..], expected[..], abs_all <= 1e-5);
        }
    }

    #[test]
    fn square_raw() {
        // 1, 10, 100, 1_000, 10_000 Hz
        for i in 0..5 {
            let freq = 10_f32.powf(i as f32);
            let sample_rate = 44100;

            let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);

            let osc = context.create_oscillator();
            osc.connect(&context.destination());
            osc.frequency().set_value(freq);
            osc.set_type(OscillatorType::Square);
            osc.start_at(0.);

            let output = context.start_rendering_sync();
            let result = output.get_channel_data(0);

            let mut expected = Vec::<f32>::with_capacity(sample_rate);
            let mut phase: f64 = 0.;
            let phase_incr = freq as f64 / sample_rate as f64;

            for _i in 0..sample_rate {
                // 0.5 belongs to the second half of the waveform
                let sample = if phase < 0.5 { 1. } else { -1. };

                expected.push(sample as f32);

                phase += phase_incr;
                if phase >= 1. {
                    phase -= 1.;
                }
            }

            assert_float_eq!(result[..], expected[..], abs_all <= 1e-10);
        }
    }

    #[test]
    fn triangle_raw() {
        // 1, 10, 100, 1_000, 10_000 Hz
        for i in 0..5 {
            let freq = 10_f32.powf(i as f32);
            let sample_rate = 44_100;

            let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);

            let osc = context.create_oscillator();
            osc.connect(&context.destination());
            osc.frequency().set_value(freq);
            osc.set_type(OscillatorType::Triangle);
            osc.start_at(0.);

            let output = context.start_rendering_sync();
            let result = output.get_channel_data(0);

            let mut expected = Vec::<f32>::with_capacity(sample_rate);
            let mut phase: f64 = 0.;
            let phase_incr = freq as f64 / sample_rate as f64;

            for _i in 0..sample_rate {
                // triangle starts a 0.
                // [0., 1.]  between [0, 0.25]
                // [1., -1.] between [0.25, 0.75]
                // [-1., 0.] between [0.75, 1]
                let mut sample = -4. * phase + 2.;

                if sample > 1. {
                    sample = 2. - sample;
                } else if sample < -1. {
                    sample = -2. - sample;
                }

                expected.push(sample as f32);

                phase += phase_incr;
                if phase >= 1. {
                    phase -= 1.;
                }
            }

            assert_float_eq!(result[..], expected[..], abs_all <= 1e-10);
        }
    }

    #[test]
    fn sawtooth_raw() {
        // 1, 10, 100, 1_000, 10_000 Hz
        for i in 0..5 {
            let freq = 10_f32.powf(i as f32);
            let sample_rate = 44_100;

            let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);

            let osc = context.create_oscillator();
            osc.connect(&context.destination());
            osc.frequency().set_value(freq);
            osc.set_type(OscillatorType::Sawtooth);
            osc.start_at(0.);

            let output = context.start_rendering_sync();
            let result = output.get_channel_data(0);

            let mut expected = Vec::<f32>::with_capacity(sample_rate);
            let mut phase: f64 = 0.;
            let phase_incr = freq as f64 / sample_rate as f64;

            for _i in 0..sample_rate {
                // triangle starts a 0.
                // [0, 1] between [0, 0.5]
                // [-1, 0] between [0.5, 1]
                let mut offset_phase = phase + 0.5;
                if offset_phase >= 1. {
                    offset_phase -= 1.;
                }
                let sample = 2. * offset_phase - 1.;

                expected.push(sample as f32);

                phase += phase_incr;
                if phase >= 1. {
                    phase -= 1.;
                }
            }

            assert_float_eq!(result[..], expected[..], abs_all <= 1e-10);
        }
    }

    #[test]
    // this one should output exactly the same thing as sine_raw
    fn periodic_wave_1f() {
        // 1, 10, 100, 1_000, 10_000 Hz
        for i in 0..5 {
            let freq = 10_f32.powf(i as f32);
            let sample_rate = 44_100;

            let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);

            let options = PeriodicWaveOptions {
                real: Some(vec![0., 0.]),
                imag: Some(vec![0., 1.]), // sine is in imaginary component
                disable_normalization: false,
            };

            let periodic_wave = context.create_periodic_wave(options);

            let osc = context.create_oscillator();
            osc.connect(&context.destination());
            osc.set_periodic_wave(periodic_wave);
            osc.frequency().set_value(freq);
            osc.set_type(OscillatorType::Sawtooth);
            osc.start_at(0.);

            let output = context.start_rendering_sync();
            let result = output.get_channel_data(0);

            let mut expected = Vec::<f32>::with_capacity(sample_rate);
            let mut phase: f64 = 0.;
            let phase_incr = freq as f64 / sample_rate as f64;

            for _i in 0..sample_rate {
                let sample = (phase * 2. * PI).sin();

                expected.push(sample as f32);

                phase += phase_incr;
                if phase >= 1. {
                    phase -= 1.;
                }
            }

            assert_float_eq!(result[..], expected[..], abs_all <= 1e-5);
        }
    }

    #[test]
    fn periodic_wave_2f() {
        // 1, 10, 100, 1_000, 10_000 Hz
        for i in 0..5 {
            let freq = 10_f32.powf(i as f32);
            let sample_rate = 44_100;

            let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);

            let options = PeriodicWaveOptions {
                real: Some(vec![0., 0., 0.]),
                imag: Some(vec![0., 0.5, 0.5]),
                // disable norm, is already tested in `PeriodicWave`
                disable_normalization: true,
            };

            let periodic_wave = context.create_periodic_wave(options);

            let osc = context.create_oscillator();
            osc.connect(&context.destination());
            osc.set_periodic_wave(periodic_wave);
            osc.frequency().set_value(freq);
            osc.set_type(OscillatorType::Sawtooth);
            osc.start_at(0.);

            let output = context.start_rendering_sync();
            let result = output.get_channel_data(0);

            let mut expected = Vec::<f32>::with_capacity(sample_rate);
            let mut phase: f64 = 0.;
            let phase_incr = freq as f64 / sample_rate as f64;

            for _i in 0..sample_rate {
                let mut sample = 0.;
                sample += 0.5 * (1. * phase * 2. * PI).sin();
                sample += 0.5 * (2. * phase * 2. * PI).sin();

                expected.push(sample as f32);

                phase += phase_incr;
                if phase >= 1. {
                    phase -= 1.;
                }
            }

            assert_float_eq!(result[..], expected[..], abs_all <= 1e-5);
        }
    }

    #[test]
    fn polyblep_isolated() {
        // @note: Only first branch of the polyblep seems to be used here.
        // May be due on the simplicity of the test itself where everything is
        // well aligned.

        // square
        {
            let mut signal = [1., 1., 1., 1., -1., -1., -1., -1.];
            let len = signal.len() as f64;
            let dt = 1. / len;

            for (index, s) in signal.iter_mut().enumerate() {
                let phase = index as f64 / len;

                *s += OscillatorRenderer::poly_blep(phase, dt, false);
                *s -= OscillatorRenderer::poly_blep((phase + 0.5) % 1., dt, false);
            }

            let expected = [0., 1., 1., 1., 0., -1., -1., -1.];

            assert_float_eq!(signal[..], expected[..], abs_all <= 0.);
        }

        // sawtooth
        {
            let mut signal = [0., 0.25, 0.75, 1., -1., -0.75, -0.5, -0.25];
            let len = signal.len() as f64;
            let dt = 1. / len;

            for (index, s) in signal.iter_mut().enumerate() {
                let phase = index as f64 / len;
                *s -= OscillatorRenderer::poly_blep((phase + 0.5) % 1., dt, false);
            }

            let expected = [0., 0.25, 0.75, 1., 0., -0.75, -0.5, -0.25];
            assert_float_eq!(signal[..], expected[..], abs_all <= 0.);
        }
    }

    #[test]
    fn osc_sub_quantum_start() {
        let freq = 1.25;
        let sample_rate = 44_100;

        let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);
        let osc = context.create_oscillator();
        osc.connect(&context.destination());
        osc.frequency().set_value(freq);
        osc.start_at(2. / sample_rate as f64);

        let output = context.start_rendering_sync();
        let result = output.get_channel_data(0);

        let mut expected = Vec::<f32>::with_capacity(sample_rate);
        let mut phase: f64 = 0.;
        let phase_incr = freq as f64 / sample_rate as f64;

        expected.push(0.);
        expected.push(0.);

        for _i in 2..sample_rate {
            let sample = (phase * 2. * PI).sin();
            phase += phase_incr;
            expected.push(sample as f32);
        }

        assert_float_eq!(result[..], expected[..], abs_all <= 1e-5);
    }

    // # Test scheduling

    #[test]
    fn osc_sub_sample_start() {
        let freq = 1.;
        let sample_rate = 96000;

        let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);
        let osc = context.create_oscillator();
        osc.connect(&context.destination());
        osc.frequency().set_value(freq);
        // start between second and third sample
        osc.start_at(1.3 / sample_rate as f64);

        let output = context.start_rendering_sync();
        let result = output.get_channel_data(0);

        let mut expected = Vec::<f32>::with_capacity(sample_rate);
        let phase_incr = freq as f64 / sample_rate as f64;
        // on first computed sample, phase is 0.7 (e.g. 2. - 1.3) * phase_incr
        let mut phase: f64 = 0.7 * phase_incr;

        expected.push(0.);
        expected.push(0.);

        for _i in 2..sample_rate {
            let sample = (phase * 2. * PI).sin();
            phase += phase_incr;
            expected.push(sample as f32);
        }

        assert_float_eq!(result[..], expected[..], abs_all <= 1e-5);
    }

    #[test]
    fn osc_sub_quantum_stop() {
        let freq = 2345.6;
        let sample_rate = 44_100;

        let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);
        let osc = context.create_oscillator();
        osc.connect(&context.destination());
        osc.frequency().set_value(freq);
        osc.start_at(0.);
        osc.stop_at(6. / sample_rate as f64);

        let output = context.start_rendering_sync();
        let result = output.get_channel_data(0);

        let mut expected = Vec::<f32>::with_capacity(sample_rate);
        let mut phase: f64 = 0.;
        let phase_incr = freq as f64 / sample_rate as f64;

        for i in 0..sample_rate {
            if i < 6 {
                let sample = (phase * 2. * PI).sin();
                phase += phase_incr;
                expected.push(sample as f32);
            } else {
                expected.push(0.);
            }
        }

        assert_float_eq!(result[..], expected[..], abs_all <= 1e-5);
    }

    #[test]
    fn osc_sub_sample_stop() {
        let freq = 8910.1;
        let sample_rate = 44_100;

        let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);
        let osc = context.create_oscillator();
        osc.connect(&context.destination());
        osc.frequency().set_value(freq);
        osc.start_at(0.);
        osc.stop_at(19.4 / sample_rate as f64);

        let output = context.start_rendering_sync();
        let result = output.get_channel_data(0);

        let mut expected = Vec::<f32>::with_capacity(sample_rate);
        let mut phase: f64 = 0.;
        let phase_incr = freq as f64 / sample_rate as f64;

        for i in 0..sample_rate {
            if i < 20 {
                let sample = (phase * 2. * PI).sin();
                phase += phase_incr;
                expected.push(sample as f32);
            } else {
                expected.push(0.);
            }
        }

        assert_float_eq!(result[..], expected[..], abs_all <= 1e-5);
    }

    #[test]
    fn osc_schedule_in_past() {
        let freq = 8910.1;
        let sample_rate = 44_100;

        let mut context = OfflineAudioContext::new(1, sample_rate, sample_rate as f32);
        let osc = context.create_oscillator();
        osc.connect(&context.destination());
        osc.frequency().set_value(freq);
        osc.start_at(-1.);

        let output = context.start_rendering_sync();
        let result = output.get_channel_data(0);

        let mut expected = Vec::<f32>::with_capacity(sample_rate);
        let mut phase: f64 = 0.;
        let phase_incr = freq as f64 / sample_rate as f64;

        for _i in 0..sample_rate {
            let sample = (phase * 2. * PI).sin();
            expected.push(sample as f32);
            phase += phase_incr;
        }

        assert_float_eq!(result[..], expected[..], abs_all <= 1e-5);
    }
}
