use eframe::{
    egui::{
        self, scroll_area::ScrollBarVisibility, Align2, Color32, Context, Image, RichText, Ui, Vec2,
    },
    App, Frame,
};
use egui_file::{DialogType, FileDialog};
use egui_float_scroller::FixedScrollbar;
use egui_toast::{Toast, ToastKind, ToastOptions, Toasts};
use std::{ffi::OsStr, fs::File, io::BufWriter, path::Path};

mod asciicast_egui;
mod cast;

use asciicast_egui::{Event, EventData, Header};
use cast::{CastFile, EventPositioned, ModificationAction};

// todo: Multiply SCROLL_WIDTH by screen size. Multiply bar length and scroll sensitivity by file length
// todo: Add general UI scaling depending on some zoom
const SCROLL_WIDTH: f32 = 20.0;
const EVENTS_PER_PAGE: usize = 50;
const COLOR_BOX_VEC: Vec2 = Vec2 { x: 30.0, y: 30.0 };
const COLOR_BOX_ROUNDING: f32 = 2.0;

fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Asciinema Editor",
        native_options,
        Box::new(|cc| {
            // Gives us image support
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(MyEguiApp::new(cc)))
        }),
    )
    .expect("eframe failed");
}

struct MyEguiApp<'a> {
    cast_file: Option<CastFile>,
    file_dialog: Option<FileDialog>,
    scroll_position: f32,
    toasts: Toasts,
    rendered_video: Option<Image<'a>>,
}

impl MyEguiApp<'_> {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            cast_file: None,
            file_dialog: None,
            scroll_position: 0.0,
            // Initialize toasts with your preferred settings
            toasts: Toasts::new()
                .anchor(Align2::LEFT_TOP, (10.0, 30.0))
                .direction(egui::Direction::TopDown),
            rendered_video: None,
        }
    }
    fn render_header(&self, ui: &mut Ui) {
        if let Some(cast_file) = &self.cast_file {
            ui.vertical(|ui| {
                let header = &cast_file.header;

                ui.heading(RichText::new("File Information:").color(Color32::LIGHT_BLUE));

                ui.group(|ui| {
                    if let Some(title) = &header.title {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Title:").strong());
                            ui.label(title);
                            ui.add_space(20.0);
                        });
                    }

                    if let Some(command) = &header.command {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Command:").strong());
                            ui.label(command);
                        });
                    }

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Version:").strong());
                        ui.label(format!("{}", header.version));
                        ui.add_space(20.0);
                        ui.label(RichText::new("Dimensions:").strong());
                        ui.label(format!("{}x{}", header.width, header.height));
                    });

                    ui.horizontal(|ui| {
                        if let Some(timestamp) = header.timestamp {
                            ui.label(RichText::new("Timestamp:").strong());
                            ui.label(format!("{}", timestamp));
                            ui.add_space(20.0);
                        }
                        if let Some(duration) = header.duration {
                            ui.label(RichText::new("Duration:").strong());
                            ui.label(format!("{}s", duration));
                        }
                    });

                    if let Some(idle_time_limit) = header.idle_time_limit {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Idle Time Limit:").strong());
                            ui.label(format!("{:.2}s", idle_time_limit));
                        });
                    }
                });

                if let Some(env) = &header.env {
                    ui.add_space(10.0);
                    ui.collapsing("Environment Variables", |ui| {
                        for (key, value) in env {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(key).strong());
                                ui.label("=");
                                ui.label(value);
                            });
                        }
                    });
                }

                if let Some(theme) = &header.theme {
                    ui.add_space(10.0);
                    ui.collapsing("Theme Settings", |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Foreground:").strong());

                            // Color preview rectangle and CSS label of foreground
                            let (_id, rect) = ui.allocate_space(COLOR_BOX_VEC);
                            ui.painter().rect_filled(rect, COLOR_BOX_ROUNDING, theme.fg);
                            ui.label(color32_to_css_rgb(&theme.fg));
                        });

                        ui.add_space(5.0);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Background:").strong());

                            // Color preview rectangle and CSS label of background
                            let (_id, rect) = ui.allocate_space(COLOR_BOX_VEC);
                            ui.painter().rect_filled(rect, COLOR_BOX_ROUNDING, theme.bg);
                            ui.label(color32_to_css_rgb(&theme.bg));
                        });

                        ui.add_space(5.0);

                        ui.label(RichText::new("Color Palette:").strong());

                        egui::Grid::new("color_palette")
                            .spacing([5.0, 5.0])
                            .show(ui, |ui| {
                                let mut col_count = 0;
                                let cols_per_row = 8; // Making this 8 allows for the clear indication if one is using 1 or 2 rows for the palette and thus 8 or 16 values for the `.cast` format

                                for color in theme.palette.iter() {
                                    let (_id, rect) = ui.allocate_space(COLOR_BOX_VEC);
                                    ui.painter().rect_filled(rect, COLOR_BOX_ROUNDING, *color);
                                    ui.label(color32_to_css_rgb(color));

                                    col_count += 1;
                                    if col_count % cols_per_row == 0 {
                                        ui.end_row(); // This serves both to end the row when 8 colors are displayed and end the grid object row so that future ui layouts are not horizontal to the grid. Since we know that the number of colors are either 8 or 16 this mod guarantees that there won't be any misplaced objects horizontally aligned to the grid and organizes the colors
                                    }
                                }
                            });
                    });
                }
            });
        }
    }

    fn render_events(&mut self, ui: &mut Ui) {
        if let Some(cast_file) = &self.cast_file {
            // Get a specified number of events starting from the scroll position passed into the memory map so that we don't need to have all the file in memory to read and edit it. This makes the editor really fast
            match cast_file.get_lines(self.scroll_position, EVENTS_PER_PAGE) {
                Ok(events) => {
                    egui::Grid::new("events_grid")
                        .num_columns(4)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            // Need enumerated line number for unique IDs for each rendered line
                            for (
                                line,
                                event_position_window,
                            ) in events.windows(3).enumerate()
                            // todo handle first element (as lines are inserted before the checks are last to first)
                            {
                                let EventPositioned {
                                    event,
                                    byte_location,
                                } = &event_position_window[1];
                                egui::ComboBox::from_id_salt(format!("button_{}", line))
                                    .selected_text("Choose...")
                                    .show_ui(ui, |ui| {
                                        // ! Double check if unwrap or 0 handles all expected conditions
                                        let order = self.cast_file.as_ref().expect("Unable to get the cast handle as mut for modification").get_order(*byte_location, event);

                                        if ui.button("Insert New Line Before This").clicked() {
                                            // todo use the result from this to inform action history to enable undo and redo
                                            let _ = self.cast_file.as_mut().expect("Unable to get the cast handle as mut for modification").action(
                                                ModificationAction::Addition(Event { time: (event_position_window[0].event.time + event.time) / 2.0, data: EventData::Output("".to_string()) }),
                                                order,
                                                &event_position_window[1],
                                                Some(&event_position_window[0]),
                                                
                                            );
                                        }
                                        
                                        if ui.button("Delete").clicked() {
                                            // todo use the result from this to inform action history to enable undo and redo
                                            let _ = self.cast_file.as_mut().expect("Unable to get the cast handle as mut for modification").action(
                                                ModificationAction::Deletion,
                                                order,
                                                &event_position_window[1],
                                                None,
                                            );
                                        }
                                    });
                                ui.label(RichText::new(event.time.to_string()).monospace());

                                ui.label(
                                    RichText::new(event.data.get_type())
                                        .color(event.data.get_color())
                                        .monospace(),
                                );

                                // Create a scrolling area with unique ID for each row
                                egui::ScrollArea::horizontal()
                                    .id_salt(format!("data_{}", line)) // Add unique ID for each scroll area
                                    .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
                                    .show(ui, |ui| {
                                        ui.add_space(4.0);
                                        ui.label(RichText::new(event.data.get_data()).monospace());
                                        ui.add_space(4.0);
                                    });

                                ui.end_row();
                            }
                        });
                    // This button will only show up if they have scrolled to the end of the file though it is always appended
                    if ui.button("Insert New Line").clicked() {}
                }
                Err(e) => {
                    self.toasts.add(Toast {
                        text: format!("Failed to get event list due to error: {}", e).into(),
                        kind: ToastKind::Error,
                        options: ToastOptions::default()
                            .duration_in_seconds(10.0)
                            .show_progress(true)
                            .show_icon(true),
                        ..Default::default()
                    });
                }
            };
        }
    }
}

impl App for MyEguiApp<'_> {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        // Crate provides a convenient interface for showing toast notifications or temporary timed popup notifications
        self.toasts.show(ctx);

        egui::TopBottomPanel::top("options").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Open button to open a file dialogue window that allows the users to select a `.cast` file
                if (ui.button("Open")).clicked() {
                    let filter = Box::new({
                        |path: &Path| -> bool { path.extension() == Some(OsStr::new("cast")) }
                    });
                    // By default open to the home directory and apply the `.cast` filter
                    let mut file_dialog =
                        FileDialog::open_file(dirs::home_dir()).show_files_filter(filter);
                    file_dialog.open();
                    self.file_dialog = Some(file_dialog);
                }

                if let Some(file) = self.cast_file.as_ref() {
                    if (ui.button("Save")).clicked() {
                        // By default open to the home directory and apply the `.cast` filter
                        let mut file_dialog = FileDialog::save_file(Some(file.file_path.clone()));
                        file_dialog.open();
                        self.file_dialog = Some(file_dialog);
                    }
                }
            });
            // This keeps open the file dialogue throughout egui updates when it has been opened by the open button and returns a opened file path buffer when a file has been selected
            if let Some(dialog) = &mut self.file_dialog {
                if dialog.show(ctx).selected() {
                    if let Some(path) = dialog.path() {
                        match dialog.dialog_type() {
                            DialogType::SelectFolder => todo!(),
                            DialogType::OpenFile => {
                                match CastFile::new(path.to_path_buf()) {
                                    Ok(cast_file) => {
                                        self.cast_file = Some(cast_file);
                                    }
                                    Err(e) => {
                                        self.toasts.add(Toast {
                                            text: format!("Failed to Create Cast Editor: {}", e)
                                                .into(),
                                            kind: ToastKind::Error,
                                            options: ToastOptions::default()
                                                .duration_in_seconds(10.0)
                                                .show_progress(true)
                                                .show_icon(true),
                                            ..Default::default()
                                        });
                                        // We need to set it to None as if it user opens another file while one's already open and there's an error we don't want to deal with a potentially unusual program state
                                        self.cast_file = None;
                                    }
                                }
                            }
                            DialogType::SaveFile => {
                                if let Some(cast_file) = self.cast_file.as_ref() {
                                    match cast_file.save_to_file(path) {
                                        Ok(()) => (),
                                        Err(e) => {
                                            self.toasts.add(Toast {
                                                text: format!("Failed to Save File: {}", e).into(),
                                                kind: ToastKind::Error,
                                                options: ToastOptions::default()
                                                    .duration_in_seconds(10.0)
                                                    .show_progress(true)
                                                    .show_icon(true),
                                                ..Default::default()
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        // todo: Check if file size even warrants a scroll bar and use it's size to inform the size of the scroll bar handle exponentially decreasing to a smaller point. Additionally allow a ron file for user settings to control settings such as minimum bar size
        if self.cast_file.is_some() {
            egui::TopBottomPanel::top("header").show(ctx, |ui| {
                self.render_header(ui);
            });

            let scrollbar = FixedScrollbar::new(&mut self.scroll_position);
            scrollbar.show_in_side_panel(ctx, "Memory Scroller");

            egui::CentralPanel::default().show(ctx, |ui| {
                self.render_events(ui);
            });
        }
    }
}

fn color32_to_css_rgb(color: &Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}
