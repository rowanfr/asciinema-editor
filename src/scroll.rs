use eframe::egui::{self, Response, Sense, Ui, Widget};

/// A fixed-size scrollbar that mutates an f32 value
pub struct FixedScrollbar<'a> {
    value: &'a mut f32,
    width: f32,
    handle_height: f32,
    scroll_sensitivity: f32,
    scroll_smoothing: bool,
}

impl<'a> FixedScrollbar<'a> {
    pub fn new(value: &'a mut f32, width: f32) -> Self {
        Self {
            value,
            width,
            handle_height: 50.0,
            scroll_sensitivity: 0.1, // Default scroll sensitivity
            scroll_smoothing: true,
        }
    }

    pub fn scroll_sensitivity(mut self, sensitivity: f32) -> Self {
        self.scroll_sensitivity = sensitivity;
        self
    }

    pub fn scroll_smoothing(mut self, scroll_smoothing: bool) -> Self {
        self.scroll_smoothing = scroll_smoothing;
        self
    }

    pub fn handle_height(mut self, handle_height: f32) -> Self {
        self.handle_height = handle_height;
        self
    }
}

impl<'a> Widget for FixedScrollbar<'a> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        let available_height = ui.available_height();

        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(self.width, available_height),
            Sense::click_and_drag(),
        );

        if response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let normalized = (pointer.y - rect.min.y) / rect.height();
                *self.value = normalized.clamp(0.0, 1.0);
            }
        }

        let scroll_delta = if self.scroll_smoothing {
            ui.input(|i| i.smooth_scroll_delta.y)
        } else {
            ui.input(|i| i.raw_scroll_delta.y)
        };

        if scroll_delta != 0.0 {
            *self.value = (*self.value
                - (scroll_delta * self.scroll_sensitivity / available_height))
                .clamp(0.0, 1.0);
        }

        // Draw the background
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(self.width, rect.height())),
            0.0,
            ui.style().visuals.extreme_bg_color,
        );

        // Draw the handle with configurable height
        let handle_height = (available_height * 0.2).min(self.handle_height);
        let handle_y = rect.min.y + (rect.height() - handle_height) * *self.value;

        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(rect.min.x, handle_y),
                egui::vec2(self.width, handle_height),
            ),
            0.0,
            ui.style().visuals.widgets.active.bg_fill,
        );

        response
    }
}
