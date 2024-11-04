use eframe::{
    egui::{self, Color32, Context, RichText, ScrollArea, Ui, Vec2},
    App, Frame,
};
use egui_file::FileDialog;
use scroll::FixedScrollbar;
use std::{ffi::OsStr, path::Path};

mod cast_parsing;
mod scroll;

use cast_parsing::{CastEditor, Event, EventCode};

// todo: Multiply SCROLL_WIDTH by screen size. Multiply bar length and scroll sensitivity by file length
const SCROLL_WIDTH: f32 = 20.0;
const EVENTS_PER_PAGE: usize = 50;
const COLOR_BOX_VEC: Vec2 = Vec2 { x: 30.0, y: 30.0 };
const COLOR_BOX_ROUNDING: f32 = 2.0;

fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Asciinema Editor",
        native_options,
        Box::new(|cc| Ok(Box::new(MyEguiApp::new(cc)))),
    )
    .expect("eframe failed");
}
#[derive(Default)]
struct MyEguiApp {
    opened_file: Option<CastEditor>,
    open_file_dialog: Option<FileDialog>,
    scroll_position: f32,
}

impl MyEguiApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }
    fn render_header(&self, ui: &mut Ui) {
        if let Some(cast_file) = &self.opened_file {
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

    fn render_events(&self, ui: &mut Ui) {
        if let Some(cast_file) = &self.opened_file {
            // Get a specified number of events starting from the scroll position passed into the memory map so that we don't need to have all the file in memory to read and edit it. This makes the editor really fast
            let events = cast_file.get_lines(self.scroll_position, EVENTS_PER_PAGE);

            for Event(timestamp, code, data) in events {
                ui.horizontal(|ui| {
                    // These are all monospaced as I find it easier to read large amounts of data
                    // todo: align these in a grid to make it easier to read
                    ui.label(RichText::new(timestamp.to_string()).monospace());

                    // Color-code different event types to visually distinguish them
                    let (event_text, color) = match code {
                        EventCode::Output => ("output", Color32::GREEN),
                        EventCode::Input => ("input", Color32::YELLOW),
                        EventCode::Marker => ("marker", Color32::BLUE),
                        EventCode::Resize => ("resize", Color32::RED),
                    };

                    ui.label(RichText::new(event_text).color(color).monospace());

                    ui.label(RichText::new(data).monospace());
                });
            }
        }
    }
}

impl App for MyEguiApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        egui::TopBottomPanel::top("options").show(ctx, |ui| {
            // Open button to open a file dialogue window that allows the users to select a `.cast` file
            if (ui.button("Open")).clicked() {
                let filter = Box::new({
                    |path: &Path| -> bool { path.extension() == Some(OsStr::new("cast")) }
                });
                // By default open to the home directory and apply the `.cast` filter
                let mut file_dialog =
                    FileDialog::open_file(dirs::home_dir()).show_files_filter(filter);
                file_dialog.open();
                self.open_file_dialog = Some(file_dialog);
            }

            // This keeps open the file dialogue throughout egui updates when it has been opened by the open button and returns a opened file path buffer when a file has been selected
            if let Some(dialog) = &mut self.open_file_dialog {
                if dialog.show(ctx).selected() {
                    if let Some(file) = dialog.path() {
                        self.opened_file = Some(CastEditor::new(file.to_path_buf()));
                    }
                }
            }
        });

        // todo: Check if file size even warrants a scroll bar and use it's size to inform the size of the scroll bar handle exponentially decreasing to a smaller point. Additionally allow a ron file for user settings to control settings such as minimum bar size
        if self.opened_file.is_some() {
            egui::TopBottomPanel::top("header").show(ctx, |ui| {
                self.render_header(ui);
            });

            egui::SidePanel::right("Scroll Bar for MMap")
                .resizable(false)
                .max_width(SCROLL_WIDTH)
                .frame(egui::Frame::none()) // Removes the panel's frame (Which removes unwanted spacing between the scroll bar and side panel)
                .show_separator_line(true) // Keeps the separator line between panels. Default behavior but I'm on the fence about look
                .show(ctx, |ui| {
                    // Custom Scroll Widget as we need to map scroll to a f32 position that we can use to determine where in MMap the file should be drawn from
                    ui.add(FixedScrollbar::new(&mut self.scroll_position, SCROLL_WIDTH));
                });

            egui::CentralPanel::default().show(ctx, |ui| {
                self.render_events(ui);
            });
        }
    }
}

fn color32_to_css_rgb(color: &Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}
