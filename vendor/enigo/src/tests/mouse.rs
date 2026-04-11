use crate::{
    Button,
    Direction::{Click, Press, Release},
    Enigo, Mouse, Settings,
    {Axis::Horizontal, Axis::Vertical},
    {Coordinate::Abs, Coordinate::Rel},
};
use std::thread;

use super::is_ci;

fn test_mouse_move(
    enigo: &mut Enigo,
    test_cases: Vec<Vec<((i32, i32), (i32, i32))>>,
    coordinate: crate::Coordinate,
    start: (i32, i32),
) {
    let delay = super::get_delay();
    thread::sleep(delay);

    let error_text = match coordinate {
        Abs => "Failed to move to",
        Rel => "Failed to relatively move to",
    };

    enigo.move_mouse(start.0, start.1, Abs).unwrap(); // Move to absolute start position
    thread::sleep(delay);

    for test_case in test_cases {
        for mouse_action in test_case {
            enigo
                .move_mouse(mouse_action.0.0, mouse_action.0.1, coordinate)
                .unwrap();
            thread::sleep(delay);
            let (x_res, y_res) = enigo.location().unwrap();
            assert_eq!(
                (mouse_action.1.0, mouse_action.1.1),
                (x_res, y_res),
                "{} {}, {}",
                error_text,
                mouse_action.0.0,
                mouse_action.0.1
            );
            thread::sleep(delay);
        }
    }
}

#[test]
// Test the move_mouse function and check it with the mouse_location function
fn unit_move_mouse_to() {
    let mut enigo = Enigo::new(&Settings::default()).unwrap();

    // Make a square of 100 pixels starting at the top left corner of the screen and
    // moving down, right, up and left
    let square = vec![
        ((0, 0), (0, 0)),
        ((0, 100), (0, 100)),
        ((100, 100), (100, 100)),
        ((100, 0), (100, 0)),
        ((0, 0), (0, 0)),
    ];
    let test_cases = vec![square];

    test_mouse_move(&mut enigo, test_cases, Abs, (0, 0));
}

#[test]
// Test the move_mouse function and check it with the mouse_location
// function
fn unit_move_mouse_rel() {
    let mut enigo = Enigo::new(&Settings::default()).unwrap();

    // Make a square of 100 pixels starting at the top left corner of the screen and
    // moving down, right, up and left
    let square = vec![
        ((0, 0), (0, 0)),
        ((0, 100), (0, 100)),
        ((100, 0), (100, 100)),
        ((0, -100), (100, 0)),
        ((-100, 0), (0, 0)),
    ];
    let test_cases = vec![square];

    test_mouse_move(&mut enigo, test_cases, Rel, (0, 0));
}

#[ignore]
#[test]
// Test the move_mouse function and check it with the mouse_location function
fn unit_move_mouse_to_boundaries() {
    let mut enigo = Enigo::new(&Settings::default()).unwrap();

    let display_size = enigo.main_display().unwrap();
    println!("Display size {} x {}", display_size.0, display_size.1);

    // Move the mouse outside of the boundaries of the screen
    let screen_boundaries = vec![
        ((-3, 8), (0, 8)),                             // Negative x coordinate
        ((8, -3), (8, 0)),                             // Negative y coordinate
        ((-30, -3), (0, 0)),                           // Negative x and y coordinates
        ((567_546_546, 20), (display_size.0 - 1, 20)), // Huge x coordinate > screen width
        ((20, 567_546_546), (20, display_size.1 - 1)), // Huge y coordinate > screen heigth
        (
            (567_546_546, 567_546_546),
            (display_size.0 - 1, display_size.1 - 1),
        ), /* Huge x and y coordinate > screen width
                                                        * and screen
                                                        * height */
        ((i32::MAX, 37), (0, 37)),              // Max x coordinate
        ((20, i32::MAX), (20, 0)),              // Max y coordinate
        ((i32::MAX, i32::MAX), (0, 0)),         // Max x and max y coordinate
        ((i32::MAX - 1, i32::MAX - 1), (0, 0)), // Max x and max y coordinate -1
        ((i32::MIN, 20), (0, 20)),              // Min x coordinate
        ((20, i32::MIN), (20, 0)),              // Min y coordinate
        ((i32::MIN, i32::MIN), (0, 0)),         // Min x and min y coordinate
        ((i32::MIN, i32::MAX), (0, 0)),         // Min x and max y coordinate
        ((i32::MAX, i32::MIN), (0, 0)),         // Max x and min y coordinate
    ];
    let test_cases = vec![screen_boundaries];

    test_mouse_move(&mut enigo, test_cases, Abs, (0, 0));
}

#[ignore]
#[test]
// Test the move_mouse function and check it with the mouse_location
// function
fn unit_move_mouse_rel_boundaries() {
    let mut enigo = Enigo::new(&Settings::default()).unwrap();

    let display_size = enigo.main_display().unwrap();
    println!("Display size {} x {}", display_size.0, display_size.1);

    // Move the mouse outside of the boundaries of the screen
    let screen_boundaries = vec![
        ((-3, 8), (0, 8)),                             // Negative x coordinate
        ((8, -10), (8, 0)),                            // Negative y coordinate
        ((-30, -3), (0, 0)),                           // Negative x and y coordinates
        ((567_546_546, 20), (display_size.0 - 1, 20)), // Huge x coordinate > screen width
        ((20, 567_546_546), (display_size.0 - 1, display_size.1 - 1)), /* Huge y coordinate >
                                                        * screen heigth */
        (
            (567_546_546, 567_546_546),
            (display_size.0 - 1, display_size.1 - 1),
        ), /* Huge x and y coordinate > screen width
            * and screen
            * height */
        ((-display_size.0, -display_size.1), (0, 0)), // Reset to (0,0)
        ((i32::MAX, 37), (0, 37)),                    // Max x coordinate
        ((20, i32::MAX), (20, 37)),                   // Max y coordinate
        ((i32::MAX, i32::MAX), (0, 0)),               // Max x and max y coordinate
        ((i32::MAX - 1, i32::MAX - 1), (0, 0)),       // Max x and max y coordinate -1
        ((i32::MIN, 20), (0, 20)),                    // Min x coordinate
        ((20, i32::MIN), (20, 0)),                    // Min y coordinate
        ((i32::MIN, i32::MIN), (0, 0)),               // Min x and min y coordinate
        ((i32::MIN, i32::MAX), (0, 0)),               // Min x and max y coordinate
        ((i32::MAX, i32::MIN), (0, 0)),               // Max x and min y coordinate
    ];
    let test_cases = vec![screen_boundaries];

    test_mouse_move(&mut enigo, test_cases, Rel, (0, 0));
}

#[test]
// Test the main_display function
// The CI's virtual display has a dimension of 1024x768. If the test is ran
// outside of the CI, we don't know the displays dimensions so we just make sure
// it is greater than 0x0.
fn unit_display_size() {
    let enigo = Enigo::new(&Settings::default()).unwrap();
    let display_size = enigo.main_display().unwrap();
    println!("Main display size: {}x{}", display_size.0, display_size.1);
    if is_ci() {
        assert_eq!((display_size.0, display_size.1), (1024, 768));
    } else {
        assert!(display_size.0 > 0);
        assert!(display_size.1 > 0);
    }
}

#[test]
// Test all the mouse buttons, make sure none of them panic
fn unit_button_click() {
    use strum::IntoEnumIterator;

    thread::sleep(super::get_delay());
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    for button in Button::iter() {
        assert_eq!(
            enigo.button(button, Press),
            Ok(()),
            "Didn't expect an error for button: {button:#?}"
        );
        assert_eq!(
            enigo.button(button, Release),
            Ok(()),
            "Didn't expect an error for button: {button:#?}"
        );
        assert_eq!(
            enigo.button(button, Click),
            Ok(()),
            "Didn't expect an error for button: {button:#?}"
        );
    }
}

#[test]
// Click each mouse button ten times, make sure none of them panic
fn unit_10th_click() {
    use strum::IntoEnumIterator;

    thread::sleep(super::get_delay());
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    for button in Button::iter() {
        for _ in 0..10 {
            assert_eq!(
                enigo.button(button, Click),
                Ok(()),
                "Didn't expect an error for button: {button:#?}"
            );
        }
    }
}

#[test]
fn unit_scroll() {
    let delay = super::get_delay();
    let mut enigo = Enigo::new(&Settings::default()).unwrap();

    // On X11 there is no scroll function but instead we repeatedly simulate a
    // keypress. This takes a long time for large values. That's why we skip them on
    // X11
    let test_cases = if cfg!(any(feature = "xdo", feature = "x11rb")) {
        vec![0, 1, 5, 100, -0, -1, -5, -100]
    } else {
        vec![0, 1, 5, 100, 57899, -57899, -0, -1, -5, -100]
    };

    for length in &test_cases {
        thread::sleep(delay);
        assert_eq!(
            enigo.scroll(*length, Horizontal),
            Ok(()),
            "Didn't expect an error when horizontally scrolling: {length}"
        );
    }
    for length in &test_cases {
        thread::sleep(delay);
        assert_eq!(
            enigo.scroll(*length, Vertical),
            Ok(()),
            "Didn't expect an error when vertically scrolling: {length}"
        );
    }
}

#[test]
// Press down and drag the mouse
fn unit_mouse_drag() {
    let delay = super::get_delay();
    let mut enigo = Enigo::new(&Settings::default()).unwrap();

    enigo.move_mouse(500, 200, Abs).unwrap();
    enigo.button(Button::Left, Press).unwrap();
    enigo.move_mouse(100, 100, Rel).unwrap();
    thread::sleep(delay);
    enigo.button(Button::Left, Release).unwrap();
}
