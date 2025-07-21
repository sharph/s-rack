use super::{AudioBuffer, AudioConfig, SharedSynthModule, SynthModule};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::any::Any;

/// Moog Filter based on
/// https://ccrma.stanford.edu/~stilti/papers/moogvcf.pdf
/// and the implementation at
/// https://www.musicdsp.org/en/latest/Filters/25-moog-vcf-variation-1.html

#[derive(Serialize, Deserialize, Clone)]
pub struct MoogFilterModule {
    id: String,
    #[serde(skip)]
    audio_in: Option<(SharedSynthModule, u8)>,
    #[serde(skip)]
    cv_in: Option<(SharedSynthModule, u8)>,
    lowpass: AudioBuffer,
    bandpass: AudioBuffer,
    highpass: AudioBuffer,
    freq: f32,
    res: f32,
    exp_amt: f32,
    state: InternalMoogFilterState,
}

impl MoogFilterModule {
    pub fn new(audio_config: &AudioConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            audio_in: None,
            cv_in: None,
            lowpass: AudioBuffer::new(Some(audio_config.buffer_size)),
            bandpass: AudioBuffer::new(Some(audio_config.buffer_size)),
            highpass: AudioBuffer::new(Some(audio_config.buffer_size)),
            freq: 0.2,
            res: 0.5,
            exp_amt: 0.5,
            state: InternalMoogFilterState::default(),
        }
    }

    pub fn get_name() -> String {
        "Moog Filter".to_string()
    }
}

#[derive(Default, Serialize, Deserialize, Clone)]
struct InternalMoogFilterState {
    f: f32,
    p: f32,
    q: f32,
    b: [f32; 5],
    freq: f32,
    res: f32,
}

impl InternalMoogFilterState {
    #[inline]
    fn calc(&mut self, input: f32, frequency: f32, res: f32) -> (f32, f32, f32) {
        if frequency != self.freq || res != self.res {
            self.freq = frequency;
            self.res = res;
            self.q = 1.0 - self.freq;
            self.p = self.freq + 0.8 * self.freq * self.q;
            self.f = self.p * 2.0 - 1.0;
            self.q = self.res * (1.0 + 0.5 * self.q * (1.0 - self.q + 5.6 * self.q * self.q));
        }
        let input = input - (self.q * self.b[4]);
        let mut t1;
        
        t1 = self.b[1];
        self.b[1] = (input + self.b[0]) * self.p - self.b[1] * self.f;
        let t2 = self.b[2];
        self.b[2] = (self.b[1] + t1) * self.p - self.b[2] * self.f;
        t1 = self.b[3];
        self.b[3] = (self.b[2] + t2) * self.p - self.b[3] * self.f;
        self.b[4] = (self.b[3] + t1) * self.p - self.b[4] * self.f;
        self.b[4] = self.b[4] - self.b[4].powi(3) * 0.166667;
        self.b[0] = input;
        self.clamp_buffers();
        (self.b[4], input - self.b[4], 3.0 * (self.b[3] - self.b[4]))
    }

    #[inline]
    fn clamp_buffers(&mut self) {
        // prevents denormals
        for x in self.b.iter_mut() {
            *x = x.min(1.0).max(-1.0);
        }
    }
}

impl SynthModule for MoogFilterModule {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        Self::get_name()
    }

    fn set_audio_config(&mut self, audio_config: &AudioConfig) {
        self.lowpass.resize(audio_config.buffer_size);
        self.bandpass.resize(audio_config.buffer_size);
        self.highpass.resize(audio_config.buffer_size);
    }

    fn get_num_inputs(&self) -> u8 {
        2
    }

    fn get_input(&self, input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        match input_idx {
            0 => Ok(self.audio_in.clone()),
            1 => Ok(self.cv_in.clone()),
            _ => Err(()),
        }
    }

    fn set_input(
        &mut self,
        input_idx: u8,
        src_module: SharedSynthModule,
        src_port: u8,
    ) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.audio_in = Some((src_module, src_port));
                Ok(())
            }
            1 => {
                self.cv_in = Some((src_module, src_port));
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.audio_in = None;
                Ok(())
            }
            1 => {
                self.cv_in = None;
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()> {
        match input_idx {
            0 => Ok(Some("Audio".to_string())),
            1 => Ok(Some("CV".to_string())),
            _ => Err(()),
        }
    }

    fn get_num_outputs(&self) -> u8 {
        3
    }

    fn get_output(&self, output_idx: u8) -> Result<AudioBuffer, ()> {
        match output_idx {
            0 => Ok(self.lowpass.clone()),
            1 => Ok(self.bandpass.clone()),
            2 => Ok(self.highpass.clone()),
            _ => Err(()),
        }
    }

    fn get_output_label(&self, output_idx: u8) -> Result<Option<String>, ()> {
        match output_idx {
            0..=2 => Ok(None),
            _ => Err(()),
        }
    }

    fn calc(&mut self) {
        AudioBuffer::with_read_many(
            vec![
                self.resolve_input(0).unwrap(),
                self.resolve_input(1).unwrap(),
            ],
            |bufs| {
                let (audio_in, cv_in) = bufs.into_iter().collect_tuple().unwrap();
                AudioBuffer::with_write_many(
                    vec![
                        self.lowpass.clone(),
                        self.bandpass.clone(),
                        self.highpass.clone(),
                    ],
                    |bufs| {
                        let (lowpass, bandpass, highpass) = bufs
                            .into_iter()
                            .map(|o| o.unwrap())
                            .collect_tuple()
                            .unwrap();
                        for idx in 0..lowpass.len() {
                            let audio = match audio_in {
                                Some(s) => s[idx],
                                None => 0.0,
                            };
                            let cv = match cv_in {
                                Some(s) => s[idx],
                                None => 0.0,
                            };
                            (lowpass[idx], highpass[idx], bandpass[idx]) = self.state.calc(
                                audio,
                                (self.freq + cv * self.exp_amt).max(0.0).min(0.9),
                                self.res.max(0.0).min(1.0),
                            );
                        }
                    },
                );
            },
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("vcf").show(ui, |ui| {
            ui.add(
                egui::Slider::new(&mut self.freq, 0.0..=1.0)
                    .orientation(egui::SliderOrientation::Vertical),
            );
            ui.add(
                egui::Slider::new(&mut self.res, 0.0..=1.0)
                    .orientation(egui::SliderOrientation::Vertical),
            );
            ui.add(
                egui::Slider::new(&mut self.exp_amt, (1.0 / 2.0_f32.powi(8))..=1.0)
                    .logarithmic(true)
                    .orientation(egui::SliderOrientation::Vertical),
            );
            ui.end_row();
            ui.label("f");
            ui.label("Q");
            ui.label("Exp.");
            ui.end_row();
        });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MoogFilterModuleV0 {
    id: String,
    #[serde(skip)]
    audio_in: Option<(SharedSynthModule, u8)>,
    #[serde(skip)]
    cv_in: Option<(SharedSynthModule, u8)>,
    buf: AudioBuffer,
    freq: f32,
    res: f32,
    exp_amt: f32,
    state: InternalMoogFilterState,
}

impl From<MoogFilterModuleV0> for MoogFilterModule {
    fn from(other: MoogFilterModuleV0) -> Self {
        let buf_size = other.buf.get().unwrap().len();
        Self {
            id: other.id,
            audio_in: other.audio_in,
            cv_in: other.cv_in,
            lowpass: other.buf,
            bandpass: AudioBuffer::new(Some(buf_size)),
            highpass: AudioBuffer::new(Some(buf_size)),
            freq: other.freq,
            res: other.res,
            exp_amt: other.exp_amt,
            state: other.state,
        }
    }
}
