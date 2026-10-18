mod ui;
use ui::app;

mod config;
mod stream;

fn main() -> iced::Result {
    env_logger::init();

    iced::application(app::Player::boot, app::Player::update, app::Player::view)
        .title("Better Stream")
        .subscription(app::Player::subscription)
        .run()
}
