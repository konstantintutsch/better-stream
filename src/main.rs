mod ui;
use ui::app;

mod stream;

#[tokio::main]
async fn main() -> iced::Result {
    iced::application(app::Player::boot, app::Player::update, app::Player::view)
        .title("Better Stream")
        .run()
}
