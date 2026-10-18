mod ui;
use ui::app;

mod config;
mod stream;

fn main() -> iced::Result {
    env_logger::init();

    let hide_title_bar = config::Config::load().hide_title_bar;

    iced::application(app::Player::boot, app::Player::update, app::Player::view)
        .title("Better Stream")
        .subscription(app::Player::subscription)
        .window(iced::window::Settings {
            maximized: true,
            decorations: !hide_title_bar,
            ..Default::default()
        })
        .run()
}
