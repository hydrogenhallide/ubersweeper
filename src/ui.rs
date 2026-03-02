use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider,
    Dialog, Entry, HeaderBar, Label, MenuButton, Orientation, ResponseType,
    ScrolledWindow, Separator, Window, gio,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::constants::Difficulty;
use crate::game::Game;
use crate::variants::{self, BoardContext, Variant};

pub struct Ubersweeper {
    game: Rc<RefCell<Game>>,
    difficulty: Rc<RefCell<Difficulty>>,
    variant: Rc<RefCell<Variant>>,
    /// Container widget that holds the current board; swapped out on reset.
    board_container: Rc<RefCell<Option<GtkBox>>>,
    /// The current board widget; variant handlers read this to do updates.
    board_widget: Rc<RefCell<Option<gtk4::Widget>>>,
    mine_label: Rc<RefCell<Option<Label>>>,
    timer_label: Rc<RefCell<Option<Label>>>,
    face_button: Rc<RefCell<Option<Button>>>,
    start_time: Rc<RefCell<Option<Instant>>>,
    timer_source: Rc<RefCell<Option<glib::SourceId>>>,
}

impl Ubersweeper {
    pub fn new() -> Self {
        let (width, height, mines) = Difficulty::Beginner.dimensions();
        Ubersweeper {
            game: Rc::new(RefCell::new(Game::new(width, height, mines, Variant::Classic))),
            difficulty: Rc::new(RefCell::new(Difficulty::Beginner)),
            variant: Rc::new(RefCell::new(Variant::Classic)),
            board_container: Rc::new(RefCell::new(None)),
            board_widget: Rc::new(RefCell::new(None)),
            mine_label: Rc::new(RefCell::new(None)),
            timer_label: Rc::new(RefCell::new(None)),
            face_button: Rc::new(RefCell::new(None)),
            start_time: Rc::new(RefCell::new(None)),
            timer_source: Rc::new(RefCell::new(None)),
        }
    }

    pub fn build_ui(&self, app: &Application) {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Übersweeper")
            .resizable(false)
            .build();

        let provider = CssProvider::new();
        provider.load_from_data(include_str!("style.css"));
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().unwrap(),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );

        let main_box = GtkBox::new(Orientation::Vertical, 15);
        main_box.set_margin_top(10);
        main_box.set_margin_bottom(15);
        main_box.set_margin_start(15);
        main_box.set_margin_end(15);

        let header = self.create_header_bar(&window);
        let info_bar = self.create_info_bar();
        header.set_title_widget(Some(&info_bar));
        window.set_titlebar(Some(&header));

        // Board container: a plain Box that holds whatever widget the variant returns.
        let container = GtkBox::new(Orientation::Vertical, 0);
        container.set_halign(gtk4::Align::Center);
        *self.board_container.borrow_mut() = Some(container.clone());
        main_box.append(&container);

        // Build the initial board.
        let board = variants::create_board(*self.variant.borrow(), &self.make_board_context());
        container.append(&board);
        *self.board_widget.borrow_mut() = Some(board);

        window.set_child(Some(&main_box));
        window.present();
    }

    fn make_board_context(&self) -> BoardContext {
        BoardContext {
            game: self.game.clone(),
            board_widget: self.board_widget.clone(),
            mine_label: self.mine_label.clone(),
            timer_label: self.timer_label.clone(),
            face_button: self.face_button.clone(),
            start_time: self.start_time.clone(),
            timer_source: self.timer_source.clone(),
        }
    }

    fn create_header_bar(&self, window: &ApplicationWindow) -> HeaderBar {
        let header = HeaderBar::new();
        header.add_css_class("flat");

        let menu = gio::Menu::new();

        // Variants grouped by difficulty tier.
        let average_tier = gio::Menu::new();
        average_tier.append(Some("Classic"),     Some("app.variant-classic"));
        average_tier.append(Some("Nested"),      Some("app.variant-nested"));
        average_tier.append(Some("Multi Mines"), Some("app.variant-multi-mines"));
        average_tier.append(Some("Minelayer"),   Some("app.variant-minelayer"));
        average_tier.append(Some("Rotation"),    Some("app.variant-rotation"));
        average_tier.append(Some("Subtract"),    Some("app.variant-subtract"));
        average_tier.append(Some("Crosswalk"),   Some("app.variant-crosswalk"));

        let hard_tier = gio::Menu::new();
        hard_tier.append(Some("Chain"),          Some("app.variant-chain"));
        hard_tier.append(Some("Drift"),          Some("app.variant-drift"));
        hard_tier.append(Some("Blindsweeper"),   Some("app.variant-blindsweeper"));
        hard_tier.append(Some("Negative Mines"), Some("app.variant-negative-mines"));
        hard_tier.append(Some("Panic"),          Some("app.variant-panic"));
        hard_tier.append(Some("Merge"),          Some("app.variant-merge"));
        hard_tier.append(Some("Marathon"),       Some("app.variant-marathon"));

        let expert_tier = gio::Menu::new();
        expert_tier.append(Some("Kudzu"),         Some("app.variant-kudzu"));
        expert_tier.append(Some("Offset"),       Some("app.variant-offset"));
        expert_tier.append(Some("Infection"),    Some("app.variant-infection"));
        expert_tier.append(Some("Relative"),     Some("app.variant-relative"));
        expert_tier.append(Some("Encrypted"),    Some("app.variant-encrypted"));
        expert_tier.append(Some("Cross-wired"),  Some("app.variant-crosswired"));

        let master_tier = gio::Menu::new();
        master_tier.append(Some("Average"),      Some("app.variant-average"));
        master_tier.append(Some("RGB"),          Some("app.variant-rgb"));
        master_tier.append(Some("3D"),           Some("app.variant-threed"));

        let variants_menu = gio::Menu::new();
        variants_menu.append_submenu(Some("Average"), &average_tier);
        variants_menu.append_submenu(Some("Hard"),    &hard_tier);
        variants_menu.append_submenu(Some("Expert"),  &expert_tier);
        variants_menu.append_submenu(Some("Master"),  &master_tier);
        menu.append_submenu(Some("Variants"), &variants_menu);

        let difficulty_section = gio::Menu::new();
        difficulty_section.append(Some("Beginner"), Some("app.beginner"));
        difficulty_section.append(Some("Intermediate"), Some("app.intermediate"));
        difficulty_section.append(Some("Expert"), Some("app.expert"));
        difficulty_section.append(Some("Custom..."), Some("app.custom"));
        menu.append_section(Some("Difficulty"), &difficulty_section);

        let help_section = gio::Menu::new();
        help_section.append(Some("How to Play"), Some("app.help"));
        help_section.append(Some("About"),       Some("app.about"));
        menu.append_section(None, &help_section);

        let menu_button = MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .build();
        header.pack_end(&menu_button);

        let app = window.application().unwrap();

        // Clone all state once; each action gets its own sub-clone.
        let game = self.game.clone();
        let difficulty = self.difficulty.clone();
        let variant = self.variant.clone();
        let board_container = self.board_container.clone();
        let board_widget = self.board_widget.clone();
        let mine_label = self.mine_label.clone();
        let timer_label = self.timer_label.clone();
        let face_button = self.face_button.clone();
        let start_time = self.start_time.clone();
        let timer_source = self.timer_source.clone();

        macro_rules! add_action {
            ($name:expr, $body:expr) => {{
                let action = gio::SimpleAction::new($name, None);
                let game_c = game.clone(); let difficulty_c = difficulty.clone();
                let variant_c = variant.clone(); let bc = board_container.clone();
                let bw = board_widget.clone(); let mine_c = mine_label.clone();
                let timer_c = timer_label.clone(); let face_c = face_button.clone();
                let start_c = start_time.clone(); let source_c = timer_source.clone();
                #[allow(clippy::redundant_closure_call)]
                action.connect_activate(move |_, _| {
                    $body(&game_c, &difficulty_c, &variant_c, &bc, &bw,
                          &mine_c, &timer_c, &face_c, &start_c, &source_c);
                });
                app.add_action(&action);
            }};
        }

        add_action!("beginner", |g, d: &Rc<RefCell<Difficulty>>, v, bc, bw, mine, timer, face, start, src| {
            *d.borrow_mut() = Difficulty::Beginner;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("intermediate", |g, d: &Rc<RefCell<Difficulty>>, v, bc, bw, mine, timer, face, start, src| {
            *d.borrow_mut() = Difficulty::Intermediate;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("expert", |g, d: &Rc<RefCell<Difficulty>>, v, bc, bw, mine, timer, face, start, src| {
            *d.borrow_mut() = Difficulty::Expert;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });

        // Custom dialog action - needs the window reference, so done separately.
        {
            let action = gio::SimpleAction::new("custom", None);
            let game_c = game.clone(); let difficulty_c = difficulty.clone();
            let variant_c = variant.clone(); let bc = board_container.clone();
            let bw = board_widget.clone(); let mine_c = mine_label.clone();
            let timer_c = timer_label.clone(); let face_c = face_button.clone();
            let start_c = start_time.clone(); let source_c = timer_source.clone();
            let window_c = window.clone();
            action.connect_activate(move |_, _| {
                Self::show_custom_dialog(
                    &window_c, &game_c, &difficulty_c, &variant_c,
                    &bc, &bw, &mine_c, &timer_c, &face_c, &start_c, &source_c,
                );
            });
            app.add_action(&action);
        }

        add_action!("variant-average", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Average;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-chain", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Chain;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-kudzu", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Kudzu;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-offset", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Offset;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-drift", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Drift;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-classic", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Classic;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-blindsweeper", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Blindsweeper;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-negative-mines", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::NegativeMines;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-multi-mines",|g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::MultiMines;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-marathon", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Marathon;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-merge", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Merge;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-panic", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Panic;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-rotation", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Rotation;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-relative", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Relative;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-rgb", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Rgb;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-subtract", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Subtract;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-threed", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Threed;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-encrypted", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Encrypted;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-minelayer", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Minelayer;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-infection", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Infection;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-nested", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Nested;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-crosswalk", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::Crosswalk;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });
        add_action!("variant-crosswired", |g, d, v: &Rc<RefCell<Variant>>, bc, bw, mine, timer, face, start, src| {
            *v.borrow_mut() = Variant::CrossWired;
            Self::reset_game_static(g, d, v, bc, bw, mine, timer, face, start, src);
        });

        {
            let action = gio::SimpleAction::new("help", None);
            let win_c = window.clone();
            action.connect_activate(move |_, _| show_help_window(&win_c));
            app.add_action(&action);
        }
        {
            let action = gio::SimpleAction::new("about", None);
            let win_c = window.clone();
            action.connect_activate(move |_, _| show_about_dialog(&win_c));
            app.add_action(&action);
        }

        header
    }

    fn show_custom_dialog(
        window: &ApplicationWindow,
        game: &Rc<RefCell<Game>>,
        difficulty: &Rc<RefCell<Difficulty>>,
        variant: &Rc<RefCell<Variant>>,
        board_container: &Rc<RefCell<Option<GtkBox>>>,
        board_widget: &Rc<RefCell<Option<gtk4::Widget>>>,
        mine_label: &Rc<RefCell<Option<Label>>>,
        timer_label: &Rc<RefCell<Option<Label>>>,
        face_button: &Rc<RefCell<Option<Button>>>,
        start_time: &Rc<RefCell<Option<Instant>>>,
        timer_source: &Rc<RefCell<Option<glib::SourceId>>>,
    ) {
        let dialog = Dialog::builder()
            .title("Custom Size")
            .transient_for(window)
            .modal(true)
            .build();
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("OK", ResponseType::Ok);

        let content = dialog.content_area();
        let grid = gtk4::Grid::new();
        grid.set_row_spacing(10);
        grid.set_column_spacing(10);
        grid.set_margin_top(10);
        grid.set_margin_bottom(10);
        grid.set_margin_start(10);
        grid.set_margin_end(10);

        let width_entry = Entry::new(); width_entry.set_text("9");
        let height_entry = Entry::new(); height_entry.set_text("9");
        let mines_entry = Entry::new(); mines_entry.set_text("10");

        grid.attach(&Label::new(Some("Width (9-50):")),  0, 0, 1, 1);
        grid.attach(&width_entry,                         1, 0, 1, 1);
        grid.attach(&Label::new(Some("Height (9-50):")), 0, 1, 1, 1);
        grid.attach(&height_entry,                        1, 1, 1, 1);
        grid.attach(&Label::new(Some("Mines (1-999):")), 0, 2, 1, 1);
        grid.attach(&mines_entry,                         1, 2, 1, 1);
        content.append(&grid);

        let game_c = game.clone(); let difficulty_c = difficulty.clone();
        let variant_c = variant.clone(); let bc = board_container.clone();
        let bw = board_widget.clone(); let mine_c = mine_label.clone();
        let timer_c = timer_label.clone(); let face_c = face_button.clone();
        let start_c = start_time.clone(); let source_c = timer_source.clone();

        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Ok {
                let width: usize = width_entry.text().parse().unwrap_or(9).clamp(9, 50);
                let height: usize = height_entry.text().parse().unwrap_or(9).clamp(9, 50);
                let max_mines = width * height - 9;
                let mines: usize = mines_entry.text().parse().unwrap_or(10).clamp(1, max_mines);
                *difficulty_c.borrow_mut() = Difficulty::Custom(width, height, mines);
                Self::reset_game_static(
                    &game_c, &difficulty_c, &variant_c, &bc, &bw,
                    &mine_c, &timer_c, &face_c, &start_c, &source_c,
                );
            }
            dialog.close();
        });

        dialog.present();
    }

    fn create_info_bar(&self) -> GtkBox {
        let info_bar = GtkBox::new(Orientation::Horizontal, 8);
        info_bar.set_halign(gtk4::Align::Center);

        let mine_label = Label::new(Some(&format!("{:03}", self.game.borrow().mine_count)));
        mine_label.add_css_class("lcd-display");
        *self.mine_label.borrow_mut() = Some(mine_label.clone());
        info_bar.append(&mine_label);

        let face_btn = Button::with_label("\u{1F642}"); // 🙂
        *self.face_button.borrow_mut() = Some(face_btn.clone());

        let game = self.game.clone();
        let difficulty = self.difficulty.clone();
        let variant = self.variant.clone();
        let board_container = self.board_container.clone();
        let board_widget = self.board_widget.clone();
        let mine_label_c = self.mine_label.clone();
        let timer_label = self.timer_label.clone();
        let face_button_c = self.face_button.clone();
        let start_time = self.start_time.clone();
        let timer_source = self.timer_source.clone();

        face_btn.connect_clicked(move |_| {
            Self::reset_game_static(
                &game, &difficulty, &variant, &board_container, &board_widget,
                &mine_label_c, &timer_label, &face_button_c, &start_time, &timer_source,
            );
        });
        info_bar.append(&face_btn);

        let timer_label = Label::new(Some("000"));
        timer_label.add_css_class("lcd-display");
        *self.timer_label.borrow_mut() = Some(timer_label.clone());
        info_bar.append(&timer_label);

        info_bar
    }

    #[allow(clippy::too_many_arguments)]
    fn reset_game_static(
        game: &Rc<RefCell<Game>>,
        difficulty: &Rc<RefCell<Difficulty>>,
        variant: &Rc<RefCell<Variant>>,
        board_container: &Rc<RefCell<Option<GtkBox>>>,
        board_widget: &Rc<RefCell<Option<gtk4::Widget>>>,
        mine_label: &Rc<RefCell<Option<Label>>>,
        timer_label: &Rc<RefCell<Option<Label>>>,
        face_button: &Rc<RefCell<Option<Button>>>,
        start_time: &Rc<RefCell<Option<Instant>>>,
        timer_source: &Rc<RefCell<Option<glib::SourceId>>>,
    ) {
        variants::stop_timer(timer_source);
        *start_time.borrow_mut() = None;

        let (width, height, mines) = difficulty.borrow().dimensions();
        let v = *variant.borrow();
        *game.borrow_mut() = Game::new(width, height, mines, v);

        // Reset labels before building the board so variants (e.g. Panic) can
        // override the timer label in their create_board without being clobbered.
        if let Some(label) = mine_label.borrow().as_ref() {
            label.set_text(&format!("{:03}", mines));
        }
        if let Some(label) = timer_label.borrow().as_ref() {
            label.set_text("000");
        }
        if let Some(btn) = face_button.borrow().as_ref() {
            btn.set_label("\u{1F642}"); // 🙂
        }

        // Swap the board widget inside its container.
        if let Some(container) = board_container.borrow().as_ref() {
            // Remove old board.
            if let Some(old) = board_widget.borrow().as_ref() {
                container.remove(old);
            }

            // Build new board. board_widget is still pointing to old value here;
            // the new board's cell handlers will read it at click-time, after we update it.
            let ctx = BoardContext {
                game: game.clone(),
                board_widget: board_widget.clone(),
                mine_label: mine_label.clone(),
                timer_label: timer_label.clone(),
                face_button: face_button.clone(),
                start_time: start_time.clone(),
                timer_source: timer_source.clone(),
            };
            let new_board = variants::create_board(v, &ctx);
            container.append(&new_board);
            *board_widget.borrow_mut() = Some(new_board);
        }
    }
}

// ---------------------------------------------------------------------------
// Help window
// ---------------------------------------------------------------------------

fn show_help_window(parent: &ApplicationWindow) {
    let win = Window::builder()
        .title("How to Play - Übersweeper")
        .transient_for(parent)
        .modal(false)
        .default_width(520)
        .default_height(720)
        .build();

    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .build();
    scrolled.set_hexpand(true);
    scrolled.set_vexpand(true);

    let content = GtkBox::new(Orientation::Vertical, 0);
    content.set_hexpand(true);
    content.set_margin_top(28);
    content.set_margin_bottom(28);
    content.set_margin_start(28);
    content.set_margin_end(28);

    // Title
    {
        let lbl = Label::new(Some("Übersweeper"));
        lbl.add_css_class("help-title");
        lbl.set_xalign(0.0);
        content.append(&lbl);
    }
    // Controls subtitle
    {
        let lbl = Label::new(Some(
            "Left-click to reveal, right-click to flag, click a revealed tile to chord.\n\
             Reveal every non-mine cell to win.",
        ));
        lbl.add_css_class("help-subtitle");
        lbl.set_xalign(0.0);
        lbl.set_wrap(true);
        lbl.set_hexpand(true);
        lbl.set_margin_top(6);
        lbl.set_margin_bottom(28);
        content.append(&lbl);
    }

    // Inner helpers - explicit parameters, not closures.
    fn tier(c: &GtkBox, name: &str) {
        let sep = Separator::new(Orientation::Horizontal);
        sep.set_margin_bottom(12);
        c.append(&sep);
        let lbl = Label::new(Some(name));
        lbl.add_css_class("help-tier");
        lbl.set_xalign(0.0);
        lbl.set_margin_bottom(10);
        c.append(&lbl);
    }

    fn entry(c: &GtkBox, name: &str, desc: &str) {
        let name_lbl = Label::new(Some(name));
        name_lbl.add_css_class("help-variant-name");
        name_lbl.set_xalign(0.0);
        c.append(&name_lbl);

        let desc_lbl = Label::new(Some(desc));
        desc_lbl.add_css_class("help-variant-desc");
        desc_lbl.set_xalign(0.0);
        desc_lbl.set_wrap(true);
        desc_lbl.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        desc_lbl.set_hexpand(true);
        desc_lbl.set_margin_top(3);
        desc_lbl.set_margin_bottom(14);
        desc_lbl.set_margin_start(2);
        c.append(&desc_lbl);
    }

    tier(&content, "Average");
    entry(&content, "Classic",        "Standard minesweeper. Each number tells you how many mines hide in the 8 surrounding cells.");
    entry(&content, "Nested",         "Every hidden cell conceals a 5×5 mini-minesweeper. Win the mini-game to safely reveal it. Lose it and the whole game ends.");
    entry(&content, "Multi Mines",    "Cells can hold 1, 2, or 3 mines. Right-click cycles flag amount (1 → 2 → 3) to match.");
    entry(&content, "Minelayer",      "The board starts fully revealed. Place mines to drive all numbers to zero - each mine reduces its 8 neighbours by 1. You have exactly as many mines as the board uses.");
    entry(&content, "Rotation",       "Every move the board rotates 90° clockwise.");
    entry(&content, "Subtract",       "Numbers show empty neighbours instead of mines - the inverse of classic logic.");
    entry(&content, "Crosswalk",      "Every move shifts one random row or column one step with wraparound. Numbers update accordingly.");

    tier(&content, "Hard");
    entry(&content, "Chain",          "You can only reveal cells within a 5×5 zone of your last click, or adjacent to already-revealed tiles.");
    entry(&content, "Drift",          "Every 3 moves all mines drift to a random adjacent unrevealed cell. Flags can become wrong as mines shift.");
    entry(&content, "Blindsweeper",   "Classic rules, but you can only see the tile under your cursor.");
    entry(&content, "Negative Mines", "About half the mines are negative - they subtract from neighbour counts instead of adding. Right-click twice to place an inverse flag.");
    entry(&content, "Panic",          "A 5-second countdown resets on every action. When time runs out, a random cell adjacent to your revealed area is clicked for you.");
    entry(&content, "Merge",          "Cells are randomly resized to 1×2, 2×1, or 2×2, giving them more neighbours.");
    entry(&content, "Marathon",       "Every N moves the bottom row is erased, everything shifts down, and a new row appears at the top. Leave anything wrong when the shift happens and you die.");

    tier(&content, "Expert");
    entry(&content, "Kudzu",          "Vine tiles (🌱) spread across the board one cell per move, hiding mines beneath them. Uncovering a tile that is on or adjacent to kudzu clears vines from the four orthogonal neighbours too.");
    entry(&content, "Offset",         "Each cell's number counts mines in a shifted 3×3 region, shown by a direction arrow - not the standard 8 neighbours.");
    entry(&content, "Infection",      "Revealing a numbered cell has a 20% chance to spawn a new mine on a random hidden neighbour, updating all nearby counts.");
    entry(&content, "Relative",       "Numbers show the difference from the cell directly above, not an absolute count. The top row shows absolute values.");
    entry(&content, "Encrypted",      "Revealed cells hide their value. Solve a maths equation to earn one peek at a cell's true number. Two failed chords lock a 3×3 area - decrypt each cell individually to unlock it.");
    entry(&content, "Cross-wired",    "The numbers on one board tell you the number of neighbouring mines on the same cell on the other.");
    tier(&content, "Master");
    entry(&content, "Average",        "Numbers show the mean mine count across all 8 neighbours - not the actual adjacency value.");
    entry(&content, "RGB",            "Three independent mine layers: Red, Green, and Blue. Hit any mine and you die. Numbers show per-channel counts; flagging one colour reveals the other two.");
    entry(&content, "3D",             "A full three-dimensional grid where each cell has up to 26 neighbours. Drag to rotate the view, scroll to zoom.");

    scrolled.set_child(Some(&content));
    win.set_child(Some(&scrolled));
    win.present();
}

// ---------------------------------------------------------------------------
// About dialog
// ---------------------------------------------------------------------------

fn show_about_dialog(parent: &ApplicationWindow) {
    let win = Window::builder()
        .title("About")
        .transient_for(parent)
        .modal(true)
        .resizable(false)
        .build();

    let header = HeaderBar::new();
    header.add_css_class("flat");
    header.set_title_widget(Some(&GtkBox::new(Orientation::Horizontal, 0)));
    win.set_titlebar(Some(&header));

    let content = GtkBox::new(Orientation::Vertical, 6);
    content.set_halign(gtk4::Align::Center);
    content.set_valign(gtk4::Align::Center);
    content.set_margin_top(24);
    content.set_margin_bottom(32);
    content.set_margin_start(40);
    content.set_margin_end(40);

    let name = Label::new(Some("Übersweeper"));
    name.add_css_class("about-name");
    content.append(&name);

    let version = Label::new(Some("0.1.0"));
    version.add_css_class("about-version");
    content.append(&version);

    let desc = Label::new(Some("Native linux minesweeper clone."));
    desc.add_css_class("about-desc");
    desc.set_margin_top(10);
    content.append(&desc);

    win.set_child(Some(&content));
    win.present();
}
