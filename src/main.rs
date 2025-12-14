extern crate sys_info;

mod which;
mod audio_engine;
mod persistence;

use propagation_endpoint::{Endpoint, EndpointKind};
use std::default::Default;
use std::option::Option::{None, Some};
use std::string::{ToString};
use winit::event::WindowEvent as WindowEventEnum;
use winit::event::Event::WindowEvent as WindowEventStruct;
use winit::event_loop::{EventLoop, ControlFlow, EventLoopBuilder, EventLoopWindowTarget};
use winit::window::WindowBuilder;
use std::iter;
use std::prelude::v1::derive;
use std::result::Result::{Err, Ok};
use std::time::Instant;
use ::egui::FontDefinitions;
use chrono::Timelike;
use egui_wgpu_backend::{RenderPass, ScreenDescriptor};
use egui_winit_platform::{Platform, PlatformDescriptor};
use wgpu::{CompositeAlphaMode, InstanceDescriptor};
use winit::event::Event::*;
use egui::{Context, Modifiers, Ui, WidgetText};
use egui_demo_lib::{DemoWindows, is_mobile};
use egui::NumExt;
use egui::text::LayoutJob;
use epi::egui::Widget;
// use sys_info::os_type;
use sysinfo::{System, Pid};

const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

const PROPAGATION_VERSION_NUMBER: &str = "0.01";

/// A custom event type for the winit app.
enum CustomEvent {
    RequestRedraw,
}

/// This is the repaint signal type that egui needs for requesting a repaint from another thread.
/// It sends the custom RequestRedraw event to the winit event loop.
struct CustomRepaintSignal(std::sync::Mutex<winit::event_loop::EventLoopProxy<CustomEvent>>);

impl epi::backend::RepaintSignal for CustomRepaintSignal {
    fn request_repaint(&self) {
        self.0.lock().unwrap().send_event(CustomEvent::RequestRedraw).ok();
    }
}

fn main() {
    let sys_info = sys_info::os_type().unwrap_or_default();
    println!("{}", sys_info);
    let which_os = which::os();
    println!("which_os = {:?}", which_os);

    let device = Endpoint::new("meme".to_string(), EndpointKind::Recording, 0);
    println!("{:?}", device);
    println!("{}", device);

    let os_infor = os_info::get();
    println!("{}: v{} {}", os_infor.os_type(), os_infor.version(), os_infor.bitness());

    let event_loop: EventLoop<CustomEvent> = EventLoopBuilder::<CustomEvent>::with_user_event().build();
    let builder = WindowBuilder::new()
        .with_decorations(true)
        .with_resizable(true)
        .with_transparent(false)
        .with_title("Propagation")
        .with_inner_size(winit::dpi::PhysicalSize {
            width: INITIAL_WIDTH,
            height: INITIAL_HEIGHT,
        });
    let window = builder.build(&event_loop).unwrap();

    let instance_desc: InstanceDescriptor = wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        .. Default::default()
    };

    let instance = wgpu::Instance::new(instance_desc);
    let surface: wgpu::Surface = unsafe { instance.create_surface(&window).unwrap() };

    // WGPU 0.11+ support force fallback (if HW implementation not supported), set it to true or false (optional).
    let adapter: wgpu::Adapter = pollster::block_on(instance.request_adapter(
        &wgpu::RequestAdapterOptions{
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })).unwrap();

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            .. Default::default()
        },
        None
    )).unwrap();

    let size = window.inner_size();
    let surface_caps = &surface.get_capabilities(&adapter);
    let surface_format = surface_caps.formats.iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(surface_caps.formats[0]);
    let mut surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: size.width,
        height: size.height,
        present_mode: surface_caps.present_modes[0],
        alpha_mode: surface_caps.alpha_modes[0],
        view_formats: vec![],
    };
    surface.configure(&device, &surface_config);

    let mut platform: Platform = Platform::new(PlatformDescriptor {
        physical_width: size.width as u32,
        physical_height: size.height as u32,
        scale_factor: window.scale_factor(),
        .. Default::default()
    });

    let mut egui_rpass = RenderPass::new(
        &device,
        surface_format,
        1);

    // let mut demo_app: DemoWindows = egui_demo_lib::DemoWindows::default();

    let mut propagation_app = PropagationWindows::default();

    let start_time = Instant::now();

    // event_loop.set_control_flow(ControlFlow::Wait);
    //
    // event_loop.run(move |event, elwt| match event {
    //     Event::WindowEvent {
    //         event: WindowEvent::CloseRequested,
    //         ..
    //     } => {
    //         println!("The close button was pressed; stopping");
    //         elwt.exit();
    //     }
    //     _ => (),
    // });

    event_loop.run(move |event, elwt: &EventLoopWindowTarget<CustomEvent>, control_flow: &mut ControlFlow| {
        platform.handle_event(&event);

        match event {
            RedrawRequested(..) => {
                platform.update_time(start_time.elapsed().as_secs_f64());

                let output_frame = match surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(wgpu::SurfaceError::Outdated) => {
                        // This error occurs when the app is minimized on Windows.
                        // Silently return here to prevent spamming the console with:
                        // "The underlying surface has changed, and therefore the swap chain must be updated"
                        return;
                    },
                    Err(e) => {
                        eprintln!("Dropped frame with error: {}", e);
                        return;
                    }
                };

                let output_view = output_frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                // Begin to draw the UI frame.
                platform.begin_frame();

                // Draw the demo application.
                // demo_app.ui(&platform.context());
                propagation_app.ui(&platform.context());

                // End the UI frame. We could now handle the output and draw the UI with the backend.
                let full_output = platform.end_frame(Some(&window));
                let paint_jobs = platform.context().tessellate(full_output.shapes);

                let mut encoder = device.create_command_encoder(
                    &wgpu::CommandEncoderDescriptor {
                        label: Some("encoder"),
                    });

                let screen_descriptor = ScreenDescriptor {
                    physical_width: surface_config.width,
                    physical_height: surface_config.height,
                    scale_factor: window.scale_factor() as f32,
                };

                let t_delta: egui::TexturesDelta = full_output.textures_delta;
                egui_rpass
                    .add_textures(&device, &queue, &t_delta)
                    .expect("add texture ok");
                egui_rpass.update_buffers(&device, &queue, &paint_jobs, &screen_descriptor);

                // Record all render passes.
                egui_rpass.execute(
                    &mut encoder,
                    &output_view,
                    &paint_jobs,
                    &screen_descriptor,
                    Some(wgpu::Color::BLACK))
                    .unwrap();

                // Submit the commands.
                queue.submit(iter::once(encoder.finish()));

                // Redraw egui
                output_frame.present();

                egui_rpass
                    .remove_textures(t_delta)
                    .expect("remove texture ok");
            },
            WindowEventStruct { event, .. } => match event {
                WindowEventEnum::Resized(size) => {
                    // Resize with 0 width and height is used by winit to signal a minimize event on Windows.
                    // See: https://github.com/rust-windowing/winit/issues/208
                    // This solves an issue where the app would panic when minimizing on Windows.
                    if size.width > 0 && size.height > 0 {
                        surface_config.width = size.width;
                        surface_config.height = size.height;
                        surface.configure(&device, &surface_config);
                    }
                },
                WindowEventEnum::CloseRequested => {
                    // elwt.exit();
                    *control_flow = ControlFlow::Exit;
                },
                _ => {},
            },
            MainEventsCleared | UserEvent(CustomEvent::RequestRedraw) => {
                window.request_redraw();
            },
            _ => {},
        }
    });
}

/// Time of day as seconds since midnight. Used for clock in demo app.
pub fn seconds_since_midnight() -> f64 {
    let time = chrono::Local::now().time();
    time.num_seconds_from_midnight() as f64 + 1e-9 * (time.nanosecond() as f64)
}

#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct PropagationWindows {
    about_is_open: bool,
    about: About,
    #[cfg_attr(feature = "serde", serde(skip))]
    audio_engine: audio_engine::AudioEngine,
    #[cfg_attr(feature = "serde", serde(skip))]
    inputs: Vec<String>,
    #[cfg_attr(feature = "serde", serde(skip))]
    outputs: Vec<String>,
    #[cfg_attr(feature = "serde", serde(skip))]
    aliases: std::collections::HashMap<String, String>,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "serde", serde(skip))]
    error_msg: Option<String>,
    // Virtual Endpoint UI State
    #[cfg_attr(feature = "serde", serde(skip))]
    virtual_endpoints: Vec<persistence::VirtualEndpointConfig>,
    #[cfg_attr(feature = "serde", serde(skip))]
    new_ve_name: String,
    #[cfg_attr(feature = "serde", serde(skip))]
    new_ve_channels: u16,
    #[cfg_attr(feature = "serde", serde(skip))]
    new_ve_type: persistence::EndpointType,
    #[cfg_attr(feature = "serde", serde(skip))]
    system: System,
    #[cfg_attr(feature = "serde", serde(skip))]
    processes: Vec<(Pid, String)>,
}

impl Default for PropagationWindows {
    fn default() -> Self {
        let mut app = Self {
            about_is_open: true,
            about: Default::default(),
            audio_engine: audio_engine::AudioEngine::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            aliases: std::collections::HashMap::new(),
            error_msg: None,
            virtual_endpoints: Vec::new(),
            new_ve_name: String::new(),
            new_ve_channels: 2,
            new_ve_type: persistence::EndpointType::Playback,
            system: System::new_all(),
            processes: Vec::new(),
        };

        // Load persisted config
        let config = persistence::AppConfig::load();
        app.aliases = config.aliases;
        app.audio_engine.latency_ms = config.latency_ms.unwrap_or(100.0);
        app.virtual_endpoints = config.virtual_endpoints;
        
        // Restore Host
        if let Some(host_name) = config.host_name {
             if !host_name.is_empty() {
                 let available = cpal::available_hosts();
                 if let Some(h) = available.into_iter().find(|h| format!("{:?}", h) == host_name) {
                     if let Err(e) = app.audio_engine.set_host(h) {
                         eprintln!("Failed to restore host: {}", e);
                     }
                 }
             }
        }

        for conn in config.connections {
            // We try to connect. If devices are missing, it will error, which we log but don't crash
            if let Err(e) = app.audio_engine.connect(&conn.input, &conn.output) {
                eprintln!("Failed to restore connection {} -> {}: {}", conn.input, conn.output, e);
            } else {
                if conn.muted {
                    app.audio_engine.set_muted(&conn.input, &conn.output, true);
                }
            }
        }
        
        app
    }
}

impl PropagationWindows {
    pub fn ui(&mut self, ctx: &Context) {
        if is_mobile(ctx) {
            self.mobile_ui(ctx);
        } else {
            self.desktop_ui(ctx);
        }
    }

    fn mobile_ui(&mut self, ctx: &Context) {
        if self.about_is_open {
            let screen_size = ctx.input(|i| i.screen_rect.size());
            let default_width = (screen_size.x - 32.0).at_most(400.0);

            let mut close = false;
            egui::Window::new(self.about.name())
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_width(default_width)
                .default_height(ctx.available_rect().height() - 46.0)
                .vscroll(true)
                .open(&mut self.about_is_open)
                .resizable(false)
                .collapsible(false)
                .show(ctx, |ui| {
                    self.about.ui(ui);
                    ui.add_space(12.0);
                    ui.vertical_centered_justified(|ui| {
                        if ui
                            .button(egui::RichText::new("Propagation").size(20.0))
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
            self.about_is_open &= !close;
        } else {
            self.mobile_top_bar(ctx);
            self.show_windows(ctx);
        }
    }

    fn mobile_top_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                let font_size = 16.5;

                // ui.menu_button(egui::RichText::new("⏷ demos").size(font_size), |ui| {
                //     ui.set_style(ui.ctx().style()); // ignore the "menu" style set by `menu_button`.
                //     self.demo_list_ui(ui);
                //     if ui.ui_contains_pointer() && ui.input(|i| i.pointer.any_click()) {
                //         ui.close_menu();
                //     }
                // });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    use egui::special_emojis::{GITHUB, TWITTER};
                    ui.hyperlink_to(
                        egui::RichText::new(TWITTER).size(font_size),
                        "https://twitter.com/massivelivefun",
                    );
                    ui.hyperlink_to(
                        egui::RichText::new(GITHUB).size(font_size),
                        "https://github.com/dezzyne/propagation",
                    );
                });
            });
        });
    }

    fn desktop_ui(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                self.file_menu_button(ui);
            });
        });

        self.show_windows(ctx);
    }

    fn show_windows(&mut self, ctx: &Context) {
        self.about.show(ctx, &mut self.about_is_open);

        // Request repaint to animate meters
        ctx.request_repaint();

        egui::Window::new("Audio Devices")
            .show(ctx, |ui| {
                if ui.button("Refresh Devices").clicked() {
                    self.update_devices();
                }

                ui.separator();
                if ui.button("Refresh Devices").clicked() {
                    self.update_devices();
                }

                if let Some(msg) = &self.error_msg {
                    ui.label(egui::RichText::new(msg).color(egui::Color32::RED));
                }

                ui.separator();
                
                ui.collapsing("Engine Configuration", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Host:");
                        egui::ComboBox::from_id_source("host_combo")
                            .selected_text(format!("{:?}", self.audio_engine.current_host_id))
                            .show_ui(ui, |ui| {
                                for host_id in cpal::available_hosts() {
                                    ui.selectable_value(&mut self.audio_engine.current_host_id, host_id, format!("{:?}", host_id));
                                }
                            });
                    });

                    ui.horizontal(|ui| {
                        ui.label("Latency (ms):");
                        ui.add(egui::Slider::new(&mut self.audio_engine.latency_ms, 10.0..=500.0).text("ms"));
                    });

                    if ui.button("Apply Settings (Restart Engine)").clicked() {
                        // Apply Host Change
                        if let Err(e) = self.audio_engine.set_host(self.audio_engine.current_host_id) {
                            self.error_msg = Some(format!("Failed to set host: {}", e));
                        } else {
                            self.error_msg = None;
                        }
                        
                        // Save Config
                        let config = persistence::AppConfig {
                            connections: self.audio_engine.get_all_connections(),
                            aliases: self.aliases.clone(),
                            host_name: Some(format!("{:?}", self.audio_engine.current_host_id)),
                            latency_ms: Some(self.audio_engine.latency_ms),
                            virtual_endpoints: self.virtual_endpoints.clone(),
                        };
                        config.save();
                        
                        // Refresh devices as streams are rebuilt
                        self.update_devices();
                    }
                });

                ui.separator();
                ui.collapsing("System Audio Integration (Virtual Endpoints)", |ui| {
                     ui.label("Define virtual endpoints here. Note: This currently only configures the application logic. Actual driver installation is required for OS visibility.");
                     
                     ui.horizontal(|ui| {
                         ui.label("Name:");
                         ui.text_edit_singleline(&mut self.new_ve_name);
                     });
                     
                     ui.horizontal(|ui| {
                         ui.label("Channels:");
                         ui.add(egui::DragValue::new(&mut self.new_ve_channels));
                     });
                     
                     ui.horizontal(|ui| {
                         ui.label("Type:");
                         ui.radio_value(&mut self.new_ve_type, persistence::EndpointType::Playback, "Playback");
                         ui.radio_value(&mut self.new_ve_type, persistence::EndpointType::Recording, "Recording");
                     });
                     
                     if ui.button("Add Virtual Endpoint").clicked() {
                         if !self.new_ve_name.is_empty() {
                             self.virtual_endpoints.push(persistence::VirtualEndpointConfig {
                                 name: self.new_ve_name.clone(),
                                 channels: self.new_ve_channels,
                                 kind: self.new_ve_type.clone(),
                             });
                             self.new_ve_name.clear();
                             
                             // Save
                             let config = persistence::AppConfig {
                                connections: self.audio_engine.get_all_connections(),
                                aliases: self.aliases.clone(),
                                host_name: Some(format!("{:?}", self.audio_engine.current_host_id)),
                                latency_ms: Some(self.audio_engine.latency_ms),
                                virtual_endpoints: self.virtual_endpoints.clone(),
                            };
                            config.save();
                         }
                     }
                     
                     ui.separator();
                     ui.label("Configured Endpoints:");
                     let mut to_remove = None;
                     for (i, ve) in self.virtual_endpoints.iter().enumerate() {
                         ui.horizontal(|ui| {
                             ui.label(format!("{} ({:?}, {}ch)", ve.name, ve.kind, ve.channels));
                             if ui.button("Remove").clicked() {
                                 to_remove = Some(i);
                             }
                         });
                     }
                     
                     if let Some(i) = to_remove {
                         self.virtual_endpoints.remove(i);
                          // Save
                             let config = persistence::AppConfig {
                                connections: self.audio_engine.get_all_connections(),
                                aliases: self.aliases.clone(),
                                host_name: Some(format!("{:?}", self.audio_engine.current_host_id)),
                                latency_ms: Some(self.audio_engine.latency_ms),
                                virtual_endpoints: self.virtual_endpoints.clone(),
                            };
                            config.save();
                     }
                });

                ui.separator();
                ui.collapsing("Application Routing", |ui| {
                     ui.label("Route specific application audio. (Windows only for now)");
                     
                     egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                         for (pid, name) in &self.processes {
                             ui.horizontal(|ui| {
                                 ui.label(format!("{} (PID: {})", name, pid));
                                 if ui.button("Route Input").clicked() {
                                     println!("Requesting loopback for PID: {}", pid);
                                     // Todo: Implement WASAPI loopback logic here
                                 }
                             });
                         }
                     });
                     
                     if ui.button("Refresh Processes").clicked() {
                         self.refresh_processes();
                     }
                });

                ui.heading("Routing Matrix");

                egui::ScrollArea::both().show(ui, |ui| {
                    egui::Grid::new("matrix_grid").striped(true).show(ui, |ui| {
                        // Header Row
                        ui.label("Input / Output");
                        for out_name in &self.outputs {
                            ui.vertical(|ui| {
                                let display_name = self.aliases.get(out_name).unwrap_or(out_name);
                                let resp = ui.label(display_name);
                                
                                // Rename context menu
                                resp.context_menu(|ui| {
                                    ui.label("Rename Output:");
                                    let mut alias = self.aliases.get(out_name).cloned().unwrap_or(out_name.clone());
                                    if ui.text_edit_singleline(&mut alias).lost_focus() {
                                        if alias.is_empty() || alias == *out_name {
                                            self.aliases.remove(out_name);
                                        } else {
                                            self.aliases.insert(out_name.clone(), alias);
                                        }
                                        // Save
                                        let config = persistence::AppConfig {
                                            connections: self.audio_engine.get_all_connections(),
                                            aliases: self.aliases.clone(),
                                            host_name: Some(format!("{:?}", self.audio_engine.current_host_id)),
                                            latency_ms: Some(self.audio_engine.latency_ms),
                                            virtual_endpoints: self.virtual_endpoints.clone(),
                                        };
                                        config.save();
                                        ui.close_menu();
                                    }
                                });

                                // Output Meter
                                if let Some(level) = self.audio_engine.get_output_level(out_name) {
                                    ui.add(egui::ProgressBar::new(level).show_percentage());
                                }
                            });
                        }
                        ui.end_row();

                        // Rows
                        for in_name in &self.inputs {
                            ui.vertical(|ui| {
                                let display_name = self.aliases.get(in_name).unwrap_or(in_name);
                                let resp = ui.label(display_name);
                                
                                // Rename context menu
                                resp.context_menu(|ui| {
                                    ui.label("Rename Input:");
                                    let mut alias = self.aliases.get(in_name).cloned().unwrap_or(in_name.clone());
                                    if ui.text_edit_singleline(&mut alias).lost_focus() {
                                        if alias.is_empty() || alias == *in_name {
                                            self.aliases.remove(in_name);
                                        } else {
                                            self.aliases.insert(in_name.clone(), alias);
                                        }
                                        // Save
                                        let config = persistence::AppConfig {
                                            connections: self.audio_engine.get_all_connections(),
                                            aliases: self.aliases.clone(),
                                            host_name: Some(format!("{:?}", self.audio_engine.current_host_id)),
                                            latency_ms: Some(self.audio_engine.latency_ms),
                                            virtual_endpoints: self.virtual_endpoints.clone(),
                                        };
                                        config.save();
                                        ui.close_menu();
                                    }
                                });

                                // Input Meter
                                if let Some(level) = self.audio_engine.get_input_level(in_name) {
                                    ui.add(egui::ProgressBar::new(level).show_percentage());
                                }
                            });

                            for out_name in &self.outputs {
                                let mut connected = self.audio_engine.is_connected(in_name, out_name);
                                
                                ui.horizontal(|ui| {
                                     // Connect Checkbox
                                    if ui.checkbox(&mut connected, "").changed() {
                                        if connected {
                                            if let Err(e) = self.audio_engine.connect(in_name, out_name) {
                                                self.error_msg = Some(e.to_string());
                                            }
                                        } else {
                                            if let Err(e) = self.audio_engine.disconnect(in_name, out_name) {
                                                self.error_msg = Some(e.to_string());
                                            }
                                        }
                                        
                                        // Save state
                                        let config = persistence::AppConfig {
                                            connections: self.audio_engine.get_all_connections(),
                                            aliases: self.aliases.clone(),
                                            host_name: Some(format!("{:?}", self.audio_engine.current_host_id)),
                                            latency_ms: Some(self.audio_engine.latency_ms),
                                            virtual_endpoints: self.virtual_endpoints.clone(),
                                        };
                                        config.save();
                                    }

                                    // Mute Button (Only if connected)
                                    if connected {
                                        let mut muted = self.audio_engine.is_muted(in_name, out_name);
                                        let btn = if muted {
                                            egui::Button::new("M").fill(egui::Color32::RED).small()
                                        } else {
                                            egui::Button::new("M").small()
                                        };
                                        
                                        if ui.add(btn).clicked() {
                                            muted = !muted;
                                            self.audio_engine.set_muted(in_name, out_name, muted);
                                            
                                            // Save state
                                            let config = persistence::AppConfig {
                                                connections: self.audio_engine.get_all_connections(),
                                                aliases: self.aliases.clone(),
                                                host_name: Some(format!("{:?}", self.audio_engine.current_host_id)),
                                                latency_ms: Some(self.audio_engine.latency_ms),
                                                virtual_endpoints: self.virtual_endpoints.clone(),
                                            };
                                            config.save();
                                        }
                                    }
                                });
                            }
                            ui.end_row();
                        }
                    });
                });

                ui.separator();
                ui.heading("Outputs");
                for output in &self.outputs {
                    ui.label(output);
                }
            });
    }

    fn update_devices(&mut self) {
        if let Ok(inputs) = self.audio_engine.list_inputs() {
            self.inputs = inputs;
        }
        if let Ok(outputs) = self.audio_engine.list_outputs() {
            self.outputs = outputs;
        }
        self.refresh_processes();
        
        for (pid, name) in &self.processes {
            self.inputs.push(format!("[App] {} ({})", name, pid));
        }
    }

    fn refresh_processes(&mut self) {
        self.system.refresh_all();
        self.processes = self.system.processes()
            .iter()
            .map(|(pid, process)| (*pid, std::path::Path::new(process.name()).display().to_string()))
            .collect();
        // Sort by name
        self.processes.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    }

    fn file_menu_button(&mut self, ui: &mut Ui) {
        let organize_shortcut =
            egui::KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, egui::Key::O);
        let reset_shortcut =
            egui::KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, egui::Key::R);

        // NOTE: we must check the shortcuts OUTSIDE of the actual "File" menu,
        // or else they would only be checked if the "File" menu was actually open!

        if ui.input_mut(|i| i.consume_shortcut(&organize_shortcut)) {
            ui.ctx().memory_mut(|mem| mem.reset_areas());
        }

        if ui.input_mut(|i| i.consume_shortcut(&reset_shortcut)) {
            ui.ctx().memory_mut(|mem| *mem = Default::default());
        }

        ui.menu_button("File", |ui: &mut Ui| {
            ui.set_min_width(220.0);
            ui.style_mut().wrap = Some(false);

            // On the web the browser controls the zoom
            #[cfg(not(target_arch = "wasm32"))]
            {
                egui::gui_zoom::zoom_menu_buttons(ui, None);
                ui.separator();
            }

            if ui
                .add(
                    egui::Button::new("Organize Windows")
                        .shortcut_text(ui.ctx().format_shortcut(&organize_shortcut)),
                )
                .clicked()
            {
                ui.ctx().memory_mut(|mem| mem.reset_areas());
                ui.close_menu();
            }

            if ui
                .add(
                    egui::Button::new("Reset egui memory")
                        .shortcut_text(ui.ctx().format_shortcut(&reset_shortcut)),
                )
                .on_hover_text("Forget scroll, positions, sizes etc")
                .clicked()
            {
                ui.ctx().memory_mut(|mem| *mem = Default::default());
                ui.close_menu();
            }
        });

        ui.menu_button("About", |ui: &mut Ui| {
            ui.set_min_width(220.0);
            ui.style_mut().wrap = Some(false);

            if ui
                .add(
                    egui::Button::new("About Propagation")
                )
                .on_hover_text("Open a window with information about Propagation")
                .clicked()
            {
                self.about_is_open = true;
                ui.close_menu();
            }
        });
    }
}

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct About {}

impl Draw for About {
    fn name(&self) -> &'static str {
        ""
    }

    fn show(&mut self, ctx: &Context, open: &mut bool) {
        egui::Window::new(self.name())
            .default_width(320.0)
            .default_height(480.0)
            .collapsible(false)
            .resizable(true)
            .open(open)
            .show(ctx, |ui| {
                self.ui(ui);
            });
    }
}

impl View for About {
    fn ui(&mut self, ui: &mut Ui) {
        ui.heading("Propagation");
        ui.label(format!("Propagation"));

        use egui::special_emojis::{GITHUB, TWITTER};
        ui.hyperlink_to(
            format!("{GITHUB} MASSIVELIVEFUN on GitHub"),
            "https://github.com/massivelivefun/",
        );
        ui.hyperlink_to(
            format!("{TWITTER} @massivelivefun"),
            "https://twitter.com/massivelivefun/",
        );

        // ui.separator();

        let os_info = os_info::get();

        ui.label(format!{
            "os name: {} {} {}",
             os_info.os_type(),
             os_info.version(),
             os_info.bitness()
        });
    }
}

/// Something to view
pub trait Draw {
    fn name(&self) -> &'static str;

    fn show(&mut self, ctx: &Context, open: &mut bool);
}

pub trait View {
    fn ui(&mut self, ui: &mut Ui);
}
