mod ui;
use ui::player;

#[tokio::main]
async fn main() -> iced::Result {
    iced::run(player::Interface::update, player::Interface::view)
}
