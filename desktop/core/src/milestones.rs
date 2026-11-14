//! The 100th thread, then each thousand (`ThreadMilestones.swift`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision { Nothing, Remember(usize), Show(usize) }

pub fn decide(count: usize, shown: Option<usize>) -> Decision {
    let reached = if count >= 1000 { count / 1000 * 1000 } else if count >= 100 { 100 } else { 0 };
    match shown {
        None if reached < 1000 => Decision::Remember(reached),
        None => Decision::Show(reached),
        Some(shown) if reached > shown => Decision::Show(reached),
        _ => Decision::Nothing,
    }
}

pub fn title(tier: usize) -> String {
    let digits = tier.to_string();
    let count: String = digits.chars().enumerate().flat_map(|(i, c)| {
        let comma = i > 0 && (digits.len() - i) % 3 == 0;
        comma.then_some(',').into_iter().chain(std::iter::once(c))
    }).collect();
    format!("That's {count} threads")
}

pub fn line(tier: usize) -> &'static str {
    match tier {
        100 => "You've got the hang of this",
        1000 => "The bear needs a lie down",
        2000 => "The lie down didn't help",
        3000 => "The bear would like a word",
        4000 => "The bear's stopped asking",
        5000 => "The bear's thinking of moving out",
        _ => "The bear's lost count, sorry",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn early_installs_start_quietly_then_each_crossing_shows_once() {
        assert_eq!(decide(0, None), Decision::Remember(0));
        assert_eq!(decide(951, None), Decision::Remember(100));
        assert_eq!(decide(100, Some(0)), Decision::Show(100));
        assert_eq!(decide(1000, Some(100)), Decision::Show(1000));
        assert_eq!(decide(1000, Some(1000)), Decision::Nothing);
        assert_eq!(decide(99, Some(1000)), Decision::Nothing);
        assert_eq!(decide(3001, Some(100)), Decision::Show(3000));
        assert_eq!(decide(3001, None), Decision::Show(3000));
        assert_eq!(title(12000), "That's 12,000 threads");
    }
}
