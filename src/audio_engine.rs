use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};
use ringbuf::wrap::caching::Caching;

// Concrete types for ringbuf 0.4.8
type AudioProducer = Caching<Arc<ringbuf::HeapRb<f32>>, true, false>;
type AudioConsumer = Caching<Arc<ringbuf::HeapRb<f32>>, false, true>;

struct InputConnection {
    output_name: String,
    producer: AudioProducer,
    muted: bool,
}

struct OutputConnection {
    input_name: String,
    consumer: AudioConsumer,
    muted: bool,
}

struct InputHandle {
    #[allow(dead_code)] // Stream must be kept alive
    stream: cpal::Stream,
    // Store connections
    connections: Arc<Mutex<Vec<InputConnection>>>,
    level: Arc<Mutex<f32>>,
}

struct OutputHandle {
    #[allow(dead_code)] // Stream must be kept alive
    stream: cpal::Stream,
    // Store connections
    connections: Arc<Mutex<Vec<OutputConnection>>>,
    level: Arc<Mutex<f32>>,
}

pub struct AudioEngine {
    host: cpal::Host,
    active_inputs: HashMap<String, InputHandle>,
    active_outputs: HashMap<String, OutputHandle>,
    // Config
    pub current_host_id: cpal::HostId,
    pub latency_ms: f32,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        let available_hosts = cpal::available_hosts();
        let mut host_id = cpal::default_host().id();
        
        // Prioritize ASIO
        for id in &available_hosts {
            if format!("{:?}", id).contains("Asio") {
                host_id = *id;
                break;
            }
        }
        
        let host = cpal::host_from_id(host_id).unwrap_or_else(|_| cpal::default_host());
        
        AudioEngine {
            host,
            active_inputs: HashMap::new(),
            active_outputs: HashMap::new(),
            current_host_id: host_id,
            latency_ms: 100.0,
        }
    }

    pub fn get_available_hosts(&self) -> Vec<cpal::HostId> {
        cpal::available_hosts()
    }

    pub fn set_host(&mut self, host_id: cpal::HostId) -> Result<(), Box<dyn Error>> {
        // If same host, do nothing
        if self.current_host_id == host_id {
            return Ok(());
        }

        // Drop all streams and clear state
        self.active_inputs.clear();
        self.active_outputs.clear();
        
        // Create new host
        self.host = cpal::host_from_id(host_id)?;
        self.current_host_id = host_id;

        Ok(())
    }

    pub fn set_latency(&mut self, latency_ms: f32) {
        self.latency_ms = latency_ms;
        // Note: Changing latency effectively requires re-building streams. 
        // For simplicity, we expect the user to re-connect or restart app, 
        // OR we can force a clear. 
        // Let's force clear to avoid inconsistent state, or just let new connections use new latency.
        // For now: New connections use new latency.
    }

    pub fn list_inputs(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut names = Vec::new();
        let devices = self.host.input_devices()?;
        for device in devices {
            if let Ok(name) = device.name() {
                names.push(name);
            }
        }
        Ok(names)
    }

    pub fn list_outputs(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut names = Vec::new();
        let devices = self.host.output_devices()?;
        for device in devices {
            if let Ok(name) = device.name() {
                names.push(name);
            }
        }
        Ok(names)
    }

    // Helper to get or create an input stream
    fn get_or_create_input(&mut self, name: &str) -> Result<&mut InputHandle, Box<dyn Error>> {
        if self.active_inputs.contains_key(name) {
            return Ok(self.active_inputs.get_mut(name).unwrap());
        }

        let device = self.host.input_devices()?
            .find(|x| x.name().map(|n| n == name).unwrap_or(false))
            .ok_or("Input device not found")?;
        
        let config: cpal::StreamConfig = device.default_input_config()?.into();
        let connections: Arc<Mutex<Vec<InputConnection>>> = Arc::new(Mutex::new(Vec::new()));
        let level: Arc<Mutex<f32>> = Arc::new(Mutex::new(0.0));

        let connections_clone = connections.clone();
        let level_clone = level.clone();

        let data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
            // Level calc
            let mut sum_sq = 0.0;
            for &sample in data {
                sum_sq += sample * sample;
            }
            if let Ok(mut l) = level_clone.try_lock() {
                *l = (sum_sq / data.len() as f32).sqrt();
            }

            // Distribute to all consumers
            if let Ok(mut conns) = connections_clone.lock() {
                for conn in conns.iter_mut() {
                    if conn.muted {
                        // If muted, we still push silence or just skip? 
                        // If we skip, buffer might underrun on read side or get out of sync.
                        // Better to push data but maybe we don't need to if the read side handles silence?
                        // Actually, simpler to push the data here, and handle mute on the output (read) side.
                        // Or push zeros here.
                        // Let's push data here, and mute on the OUTPUT side where mixing happens. 
                        // Wait, InputConnection pushes to Output. 
                        // If we silence here, it affects that specific link. 
                        // Let's just push normally here, and rely on the Output side to check mute status?
                        // But InputConnection `muted` flag is here. 
                        // Let's push silence if muted.
                        // Creating a zero vector is expensive.
                        // Let's just push the data here. The architecture is Input -> Buffer -> Output.
                        // The mute flag should ideally be checked at the Output side (Reader) to avoid pushing silence into a buffer.
                        // However, `InputConnection` is owned by InputHandle.
                        // We need access to the mute state.
                        conn.producer.push_slice(data); // Push always to keep sync
                    } else {
                        conn.producer.push_slice(data);
                    }
                }
            }
        };

        let stream = device.build_input_stream(
            &config,
            data_fn,
            |e| eprintln!("Input stream error: {}", e),
            None
        )?;
        stream.play()?;

        let handle = InputHandle {
            stream,
            connections,
            level,
        };
        
        self.active_inputs.insert(name.to_string(), handle);
        Ok(self.active_inputs.get_mut(name).unwrap())
    }

    // Helper to get or create an output stream
    fn get_or_create_output(&mut self, name: &str) -> Result<&mut OutputHandle, Box<dyn Error>> {
        if self.active_outputs.contains_key(name) {
            return Ok(self.active_outputs.get_mut(name).unwrap());
        }

        let device = self.host.output_devices()?
            .find(|x| x.name().map(|n| n == name).unwrap_or(false))
            .ok_or("Output device not found")?;
        
        let config: cpal::StreamConfig = device.default_output_config()?.into();
        let connections: Arc<Mutex<Vec<OutputConnection>>> = Arc::new(Mutex::new(Vec::new()));
        let level: Arc<Mutex<f32>> = Arc::new(Mutex::new(0.0));

        let connections_clone = connections.clone();
        let level_clone = level.clone();
        let _channels = config.channels as usize;

        let data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // Zero out buffer first
            for sample in data.iter_mut() {
                *sample = 0.0;
            }

            // Mix consumers
            if let Ok(mut conns) = connections_clone.lock() {
                // Temporary buffer for mixing
                let mut temp_buf = vec![0.0; data.len()]; 
                
                for conn in conns.iter_mut() {
                    let pulled = conn.consumer.pop_slice(&mut temp_buf);
                    
                    if !conn.muted {
                         // Add to main buffer
                        for i in 0..pulled {
                            data[i] += temp_buf[i];
                        }
                    }
                    // If muted, we popped the data (to keep ringbuf drained/synced) but didn't add it.
                }
            }

            // Level calc
            let mut sum_sq = 0.0;
            for &sample in data.iter() {
                sum_sq += sample * sample;
            }
             if let Ok(mut l) = level_clone.try_lock() {
                *l = (sum_sq / data.len() as f32).sqrt();
            }
        };

        let stream = device.build_output_stream(
            &config,
            data_fn,
            |e| eprintln!("Output stream error: {}", e),
            None
        )?;
        stream.play()?;

        let handle = OutputHandle {
            stream,
            connections,
            level,
        };
        
        self.active_outputs.insert(name.to_string(), handle);
        Ok(self.active_outputs.get_mut(name).unwrap())
    }

    pub fn connect(&mut self, input_name: &str, output_name: &str) -> Result<(), Box<dyn Error>> {
        // Ensure streams exist
        self.get_or_create_input(input_name)?;
        self.get_or_create_output(output_name)?;

        // Check if already connected to avoid duplicates
        if self.is_connected(input_name, output_name) {
            return Ok(());
        }

        // Create RingBuffer
        // Calculate buffer size based on latency
        // frames = sample_rate * (ms / 1000)
        // Assume 48kHz for now (todo: get actual sample rate from device config)
        let latency_frames = (48000.0 * (self.latency_ms / 1000.0)) as usize;
        let rb = HeapRb::<f32>::new(latency_frames * 2); // *2 for stereo safety margin
        let (producer, consumer) = rb.split();

        // Add to lists
        {
            let input = self.active_inputs.get(input_name).ok_or("Input init failed")?;
            input.connections.lock().unwrap().push(InputConnection {
                output_name: output_name.to_string(),
                producer,
                muted: false,
            });
        }
        {
            let output = self.active_outputs.get(output_name).ok_or("Output init failed")?;
            output.connections.lock().unwrap().push(OutputConnection {
                input_name: input_name.to_string(),
                consumer,
                muted: false,
            });
        }

        Ok(())
    }

    pub fn disconnect(&mut self, input_name: &str, output_name: &str) -> Result<(), Box<dyn Error>> {
        if let Some(input) = self.active_inputs.get(input_name) {
            let mut conns = input.connections.lock().unwrap();
            conns.retain(|c| c.output_name != output_name);
        }

        if let Some(output) = self.active_outputs.get(output_name) {
            let mut conns = output.connections.lock().unwrap();
            conns.retain(|c| c.input_name != input_name);
        }
        Ok(())
    }

    pub fn is_connected(&self, input_name: &str, output_name: &str) -> bool {
        if let Some(input) = self.active_inputs.get(input_name) {
             if let Ok(conns) = input.connections.lock() {
                 return conns.iter().any(|c| c.output_name == output_name);
             }
        }
        false
    }

    pub fn set_muted(&mut self, input_name: &str, output_name: &str, muted: bool) {
        // We only really need to mute on the Output side (the mixer side) to silence audio.
        // But for consistency we can update both, or just one.
        // Updating Output side is sufficient for the mixing logic I wrote above.
        if let Some(output) = self.active_outputs.get(output_name) {
             if let Ok(mut conns) = output.connections.lock() {
                 if let Some(c) = conns.iter_mut().find(|c| c.input_name == input_name) {
                     c.muted = muted;
                 }
             }
        }
        // Update input side too just for state consistency if we ever use it
        if let Some(input) = self.active_inputs.get(input_name) {
             if let Ok(mut conns) = input.connections.lock() {
                 if let Some(c) = conns.iter_mut().find(|c| c.output_name == output_name) {
                     c.muted = muted;
                 }
             }
        }
    }

    pub fn is_muted(&self, input_name: &str, output_name: &str) -> bool {
        if let Some(output) = self.active_outputs.get(output_name) {
             if let Ok(conns) = output.connections.lock() {
                 if let Some(c) = conns.iter().find(|c| c.input_name == input_name) {
                     return c.muted;
                 }
             }
        }
        false
    }

    // Helper for UI to get levels
    pub fn get_input_level(&self, name: &str) -> Option<f32> {
        self.active_inputs.get(name)
            .and_then(|h| h.level.try_lock().ok().map(|l| *l))
    }

    pub fn get_output_level(&self, name: &str) -> Option<f32> {
        self.active_outputs.get(name)
            .and_then(|h| h.level.try_lock().ok().map(|l| *l))
    }

    pub fn get_all_connections(&self) -> Vec<crate::persistence::ConnectionConfig> {
        let mut connections = Vec::new();
        for (input_name, input) in &self.active_inputs {
             if let Ok(conns) = input.connections.lock() {
                for conn in conns.iter() {
                    connections.push(crate::persistence::ConnectionConfig {
                        input: input_name.clone(),
                        output: conn.output_name.clone(),
                        muted: conn.muted,
                    });
                }
            }
        }
        connections
    }
}
