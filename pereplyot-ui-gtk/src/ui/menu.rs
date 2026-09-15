/// The hamburger menu model: four actions total, so a plain `gio::Menu` — rather than the
/// hand-built popover the wider house style favors for menus with more items — keeps this
/// small and gets a radio-styled Theme submenu for free from the stateful `win.theme` action
/// (see `window::install_actions`).
pub fn build() -> gio::Menu {
    let menu = gio::Menu::new();

    let file_section = gio::Menu::new();
    file_section.append(Some("Open…"), Some("win.open"));
    menu.append_section(None, &file_section);

    let theme_section = gio::Menu::new();
    theme_section.append(Some("System"), Some("win.theme::system"));
    theme_section.append(Some("Light"), Some("win.theme::light"));
    theme_section.append(Some("Dark"), Some("win.theme::dark"));
    let theme_submenu = gio::Menu::new();
    theme_submenu.append_section(None, &theme_section);
    menu.append_submenu(Some("Theme"), &theme_submenu);

    let about_section = gio::Menu::new();
    about_section.append(Some("Changelog"), Some("win.changelog"));
    about_section.append(Some("About Pereplyot"), Some("win.about"));
    menu.append_section(None, &about_section);

    menu
}
