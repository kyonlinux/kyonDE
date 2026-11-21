use gtk4 as gtk;

use crate::{desktop, panel};

pub fn build(app: &gtk::Application) {
    panel::build(app);
    desktop::build(app);
}
