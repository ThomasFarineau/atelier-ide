//! Merge conflict markers: parsing a conflicted file and writing it back once resolved.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Ours,
    Theirs,
    OursThenTheirs,
    TheirsThenOurs,
}

#[derive(Clone, Debug)]
pub struct Conflict {
    pub ours_label: String,
    pub theirs_label: String,
    pub ours: String,
    pub theirs: String,
    /// Common ancestor, present with `merge.conflictStyle=diff3`.
    pub base: Option<String>,
    pub choice: Option<Choice>,
    /// The block exactly as found in the file, written back while unresolved.
    raw: String,
}

#[derive(Clone, Debug)]
pub enum Segment {
    Text(String),
    Conflict(Conflict),
}

fn marker<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(prefix)?;
    (rest.is_empty() || rest.starts_with([' ', '\r', '\n'])).then(|| rest.trim())
}

pub fn parse(text: &str) -> Vec<Segment> {
    #[derive(PartialEq)]
    enum Part {
        Ours,
        Base,
        Theirs,
    }
    let mut segments = Vec::new();
    let mut plain = String::new();
    let mut current: Option<(Conflict, Part)> = None;
    for line in text.split_inclusive('\n') {
        match &mut current {
            None => match marker(line, "<<<<<<<") {
                Some(label) => {
                    if !plain.is_empty() {
                        segments.push(Segment::Text(std::mem::take(&mut plain)));
                    }
                    let conflict = Conflict {
                        ours_label: label.into(),
                        theirs_label: String::new(),
                        ours: String::new(),
                        theirs: String::new(),
                        base: None,
                        choice: None,
                        raw: line.into(),
                    };
                    current = Some((conflict, Part::Ours));
                }
                None => plain.push_str(line),
            },
            Some((conflict, part)) => {
                conflict.raw.push_str(line);
                if *part != Part::Theirs && marker(line, "|||||||").is_some() {
                    *part = Part::Base;
                    conflict.base = Some(String::new());
                } else if *part != Part::Theirs && marker(line, "=======").is_some() {
                    *part = Part::Theirs;
                } else if *part == Part::Theirs
                    && let Some(label) = marker(line, ">>>>>>>")
                {
                    conflict.theirs_label = label.into();
                    let (conflict, _) = current.take().expect("conflict in progress");
                    segments.push(Segment::Conflict(conflict));
                } else {
                    match part {
                        Part::Ours => conflict.ours.push_str(line),
                        Part::Base => conflict.base.get_or_insert_default().push_str(line),
                        Part::Theirs => conflict.theirs.push_str(line),
                    }
                }
            }
        }
    }
    // An unterminated block is not a conflict: keep it as text.
    if let Some((conflict, _)) = current {
        plain.push_str(&conflict.raw);
    }
    if !plain.is_empty() {
        segments.push(Segment::Text(plain));
    }
    segments
}

pub fn conflicts(segments: &[Segment]) -> usize {
    segments
        .iter()
        .filter(|s| matches!(s, Segment::Conflict(_)))
        .count()
}

pub fn resolved(segments: &[Segment]) -> bool {
    segments
        .iter()
        .all(|s| !matches!(s, Segment::Conflict(c) if c.choice.is_none()))
}

/// The file content: resolved blocks replaced by the chosen side, others left with their markers.
pub fn render(segments: &[Segment]) -> String {
    let mut out = String::new();
    for segment in segments {
        match segment {
            Segment::Text(text) => out.push_str(text),
            Segment::Conflict(c) => match c.choice {
                None => out.push_str(&c.raw),
                Some(Choice::Ours) => out.push_str(&c.ours),
                Some(Choice::Theirs) => out.push_str(&c.theirs),
                Some(Choice::OursThenTheirs) => {
                    out.push_str(&c.ours);
                    out.push_str(&c.theirs);
                }
                Some(Choice::TheirsThenOurs) => {
                    out.push_str(&c.theirs);
                    out.push_str(&c.ours);
                }
            },
        }
    }
    out
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/git/conflict/tests.rs"]
mod tests;
