use cpal::traits::DeviceTrait;
use cpal::traits::HostTrait;
use cpal::traits::StreamTrait;
use cpal::SupportedBufferSize;
use std::f64::consts::PI;
use std::iter::zip;
use std::sync::Arc;
use std::sync::RwLock;
use std::thread::sleep;
use std::time;
use uuid;

fn main() {
    println!("Hello, world!");
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No output device available!");
    let mut supported_configs_range = device
        .supported_output_configs()
        .expect("error while querying configs");
    let supported_config = supported_configs_range
        .next()
        .expect("no supported config")
        .with_sample_rate(cpal::SampleRate(48000));
    let buffer_size = match supported_config.buffer_size() {
        SupportedBufferSize::Range { min: _, max: _ } => 2048,
        SupportedBufferSize::Unknown => 4096,
    };
    let sample_rate = supported_config.sample_rate().0;
    let mut channels = <u8>::try_from(supported_config.channels()).unwrap();
    channels = 2;
    let lfo = Arc::new(RwLock::new(DCOModule::new(buffer_size, sample_rate)));
    lfo.write().unwrap().val = -9.0;
    let osc = Arc::new(RwLock::new(DCOModule::new(buffer_size, sample_rate)));
    let output = Arc::new(RwLock::new(OutputModule::new(
        buffer_size,
        sample_rate,
        channels,
    )));
    println!(
        "Sample rate: {}, Buffer size: {}, channels: {}",
        sample_rate, buffer_size, channels
    );
    connect(lfo.clone(), 0, osc.clone(), 0).unwrap();
    connect(osc.clone(), 0, output.clone(), 0).unwrap();
    connect(osc.clone(), 0, output.clone(), 1).unwrap();
    let mut src_buf_pos: usize = 0;
    let plan = plan_execution(output.clone());
    let stream = device
        .build_output_stream(
            &cpal::StreamConfig {
                channels: channels.into(),
                sample_rate: cpal::SampleRate(sample_rate),
                buffer_size: cpal::BufferSize::Fixed(buffer_size.try_into().unwrap()),
            },
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let out = output.clone();
                let src_buf = &out.read().unwrap().bufs.clone();
                for dst_buf_pos in 0..data.len() {
                    let channel = dst_buf_pos % <usize>::from(channels);
                    data[dst_buf_pos] = src_buf[channel][src_buf_pos];
                    if dst_buf_pos % <usize>::from(channels) == <usize>::from(channels) - 1 {
                        src_buf_pos += 1;
                        if src_buf_pos >= buffer_size {
                            execute(&plan);
                            src_buf_pos = 0;
                        }
                    }
                }
            },
            move |_err| {},
            None,
        )
        .unwrap();
    stream.play().unwrap();
    sleep(time::Duration::from_secs(10));
}

fn execute(plan: &Vec<SharedSynthModule>) {
    for ssm in plan {
        ssm.write().unwrap().calc();
    }
}

fn plan_execution(output: SharedSynthModule) -> Vec<SharedSynthModule> {
    let mut execution_list: Vec<(SharedSynthModule, bool)> = vec![(output, false)];
    let mut to_concat: Vec<(SharedSynthModule, bool)> = Vec::new();
    loop {
        if let Some((idx, (to_search, _searched))) = execution_list
            .iter()
            .enumerate()
            .filter(|(_idx, (_sm, searched))| !searched)
            .next()
        {
            for input in to_search.read().unwrap().get_inputs() {
                if let Some((input, _)) = input {
                    if !execution_list.iter().any(|(to_compare, _)| {
                        input.read().unwrap().get_id() == to_compare.read().unwrap().get_id()
                    }) {
                        to_concat.push((input, false));
                        break;
                    }
                }
            }
            execution_list[idx].1 = true;
        } else {
            break;
        }
        execution_list.append(&mut to_concat);
    }
    execution_list
        .iter()
        .rev()
        .map(|(sm, _searched)| {
            println!(
                "{} {}",
                sm.read().unwrap().get_name(),
                sm.read().unwrap().get_id()
            );
            sm.clone()
        })
        .collect()
}

fn connect(
    src_module: SharedSynthModule,
    src_port: u8,
    sink_module: SharedSynthModule,
    sink_port: u8,
) -> Result<(), ()> {
    let mut sink_module_write = sink_module.write().unwrap();
    sink_module_write.set_input(sink_port, src_module.clone(), src_port)?;
    Ok(())
}

type ControlVoltage = f32;

trait SynthModule {
    fn get_name(&self) -> String;
    fn get_id(&self) -> String;
    fn calc(&mut self);
    fn get_num_inputs(&self) -> u8;
    fn get_num_outputs(&self) -> u8;
    fn get_input(&self, input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()>;
    fn get_inputs(&self) -> Vec<Option<(SharedSynthModule, u8)>>;
    fn get_output(&self, output_idx: u8) -> Result<&[ControlVoltage], ()>;
    fn set_input(
        &mut self,
        input_idx: u8,
        src_module: SharedSynthModule,
        src_port: u8,
    ) -> Result<(), ()>;
}
type SharedSynthModule = Arc<RwLock<dyn SynthModule + Send + Sync>>;

struct DCOModule {
    id: String,
    val: ControlVoltage,
    input: Option<(SharedSynthModule, u8)>,
    sample_rate: u32,
    buf: Box<[ControlVoltage]>,
    pos: f64,
}

impl DCOModule {
    fn new(buffer_size: usize, sample_rate: u32) -> DCOModule {
        DCOModule {
            id: uuid::Uuid::new_v4().into(),
            input: None,
            val: 0.0,
            sample_rate,
            buf: (0..buffer_size).map(|_| 0.0).collect(),
            pos: 0.0,
        }
    }

    fn get_freq_in_hz(&self, buf: Option<&[ControlVoltage]>, i: usize) -> f64 {
        match buf {
            Some(buf) => 440.0 * (2.0_f64.powf(<f64>::from(buf[i]) + <f64>::from(self.val))),
            None => 440.0 * (2.0_f64.powf(<f64>::from(self.val))),
        }
    }
}

impl SynthModule for DCOModule {
    fn get_name(&self) -> String {
        "DCO".to_string()
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_output(&self, output_idx: u8) -> Result<&[ControlVoltage], ()> {
        if output_idx == 0 {
            return Ok(&self.buf);
        }
        Err(())
    }

    fn calc(&mut self) {
        let mut input_buf: Option<&[ControlVoltage]> = None;
        let input_module;
        if let Some((input, port)) = &self.input {
            input_module = input.read().unwrap();
            input_buf = Some(input_module.get_output(*port).unwrap());
        }
        for i in 0..self.buf.len() {
            self.pos = self.pos + (self.get_freq_in_hz(input_buf, i) / (self.sample_rate as f64));
            self.buf[i] = (self.pos * PI * 2.0).sin() as ControlVoltage
        }
        self.pos = self.pos % 1.0;
    }

    fn get_num_outputs(&self) -> u8 {
        1
    }

    fn get_input(&self, output_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        if output_idx == 0 {
            return Ok(self.input.clone());
        }
        Err(())
    }

    fn get_inputs(&self) -> Vec<Option<(SharedSynthModule, u8)>> {
        vec![self.input.clone()]
    }

    fn get_num_inputs(&self) -> u8 {
        1
    }

    fn set_input(
        &mut self,
        input_idx: u8,
        src_module: SharedSynthModule,
        src_port: u8,
    ) -> Result<(), ()> {
        if input_idx == 0 {
            self.input = Some((src_module, src_port));
            return Ok(());
        }
        Err(())
    }
}

struct OutputModule {
    id: String,
    bufs: Box<[Box<[ControlVoltage]>]>,
    inputs: Box<[Option<(SharedSynthModule, u8)>]>,
}

impl OutputModule {
    fn new(buffer_size: usize, _sample_rate: u32, channels: u8) -> OutputModule {
        OutputModule {
            id: uuid::Uuid::new_v4().into(),
            bufs: (0..channels)
                .map(|_| (0..buffer_size).map(|_| 0.0).collect())
                .collect(),
            inputs: (0..channels).map(|_| None).collect(),
        }
    }
}

impl SynthModule for OutputModule {
    fn get_name(&self) -> String {
        "Output".to_string()
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn calc(&mut self) {
        for (input, buf) in zip(self.inputs.iter_mut(), self.bufs.iter_mut()) {
            match input {
                Some((module, port)) => {
                    let input_module = module.read().unwrap();
                    buf.copy_from_slice(input_module.get_output(*port).unwrap());
                }
                None => buf.fill(0.0),
            }
        }
    }

    fn get_num_outputs(&self) -> u8 {
        0
    }

    fn get_output(&self, _: u8) -> Result<&[ControlVoltage], ()> {
        Err(())
    }

    fn get_num_inputs(&self) -> u8 {
        self.inputs.len().try_into().unwrap()
    }

    fn get_input(&self, idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        if <usize>::from(idx) > self.inputs.len() {
            return Err(());
        }
        Ok(self.inputs[<usize>::from(idx)].clone())
    }

    fn get_inputs(&self) -> Vec<Option<(SharedSynthModule, u8)>> {
        (0..self.get_num_inputs())
            .map(|n| self.get_input(n).unwrap())
            .collect()
    }

    fn set_input(
        &mut self,
        input_idx: u8,
        src_module: SharedSynthModule,
        src_port: u8,
    ) -> Result<(), ()> {
        if input_idx >= self.get_num_inputs() {
            return Err(());
        }
        let src_module_read = src_module.read().unwrap();
        if src_port >= src_module_read.get_num_outputs() {
            return Err(());
        }
        self.inputs[<usize>::from(input_idx)] = Some((src_module.clone(), src_port));
        Ok(())
    }
}
