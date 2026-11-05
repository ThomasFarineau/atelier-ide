//! The commands of the quality gates (MCP prompts): what the AI is asked to do with the findings
//! of each tool, and `ub-mode`, the whole practice the gates belong to.

use serde_json::{Value, json};

const CRAPPER_RULES: &str = "\
- Reduce each function under CRAP 10 by splitting it, by testing it, or both.
- Consider whether case statements are worth reducing.";

const VERIFIER_RULES: &str = "\
- A split must not leave pieces that only fragment the code.
- Merge a function into its caller when it is defined once, called from one place only, and its CRAP plus its caller's is under 8.
- Merge only what pastes as it is: not into a closure, not a body that exits early (`?`, `let ... else`, a `return` before its end).
- Count every mention, tests included: a call from a test, a use as a value or in a macro keeps the function.
- Keep a named step of a sequence or a block that means something on its own; merge the rest.";

const DRYER_RULES: &str = "\
- Eliminate the duplication that ranks 0.8 or above.";

const MUTATOR_RULES: &str = "\
- Cover the uncovered.
- Kill the survivors.
- Delete dead code.";

/// How every finding is fixed.
const RIGHT: &str = "\
## Fix Root Causes

* Reproduce it.
* Fix the cause.
* A guard that only hides the crash is not a fix.

## Sequence work into verifiable units

* Finish one small unit and check it.
* Then start the next.

## Test Behavior, Not Implementation

* The test calls the code the way its user does and checks the result that user would see.
* If the test would still pass when every function returned nothing, throw it out.

## Refactoring

* Pin what the code does.
* Change the structure.
* Prove the behavior did not change.";

const PRACTICE: &str = "\
# Practice

## Keep a Log

* Keep a log of every action you take and the reason you took it.
* Report that log upon completion.

## Guard the Context Window

* Keep the main thread small.
* Send the bulk elsewhere, and keep the summary.

## Worktree Cleanup

* Find what is safe to remove, confirm nothing still in use needs it, then reclaim the disk.
* Uncommitted work waits for a decision.

## Commit and Push

* Commit often, in small ordered commits.
* Each commit says why, what changed, and how you proved it.
* Push to the main line at the end.";

const MAKE_IT_WORK: &str = "\
# First Make it Work

## Laziness Protocol

* One decision lives in one place.
* Ship the smallest edit that solves the problem.

## Foundational Thinking

* Get the data shape right before you write the logic.
* Do first whatever helps every later phase.
* If a concurrent writer would matter, do not share state.

## TDD

* Write the failing test before you change the code.
* See it fail for the reason you expect.
* Make the smallest change that makes it pass.
* If an honest test is impractical, do not write a bad one.

## Redesign From First Principles

* Do not bolt a new requirement onto the old design.
* Ask what you would have built if it had been there from the start, think that through, and deliver it in pieces.

## Attack the Premise

* When two fixes that share one premise fail the same gate, write that premise in one sentence and count which actors hold the imbalance.
* Remove the assignment.
* Do not start another fix until you have done that.

## Subtract Before You Add

* Take complexity out before you add anything.

## Outcome-Oriented Execution

* Aim at the end state you can check.
* A break in the middle is allowed when you planned it and can undo it.

## Build the Lever

* If the work is not obvious at a glance, build the tool that does it.
* Do one unit by hand, then make the tool match that unit.

## Architecture

### Model the Domain

* Put the domain in a structure that makes the illegal states impossible.
* Maintain a glossary of domain terms.
* If the code is already clear and local, leave it.

### Boundary Discipline

* Check the data where it enters.
* Inside, trust it.
* Keep the business logic in a pure function.

### Type System Discipline

* Make the illegal value impossible to write.
* Parse outside data once, where it comes in.
* Do not silence the compiler.

### Make Operations Idempotent

* Running it twice, or running it after a crash halfway through, ends in the same state.

### Separate Before Serializing Shared State

* If they do not need the same mutable thing, give each its own.
* If they do, allow one writer, and enforce that in the structure.";

/// The whole practice: the principles, then the gates (the Atelier tools) and how to fix.
fn ub_mode() -> String {
    format!(
        "Follow the reduced principles for every change of this session. Every change follows: First Make it Work, then Then Make it Right, then Follow Practice.\n\n\
         {MAKE_IT_WORK}\n\n\
         # Then Make it Right\n\n\
         {RIGHT}\n\n\
         ## Crapper\n\n* Use the Atelier tool `crapper`.\n{CRAPPER_RULES}\n\n\
         ## Verifier\n\n* Use the Atelier tool `verifier` after the crapper.\n{VERIFIER_RULES}\n\n\
         ## Dryer\n\n* Use the Atelier tool `dryer`.\n{DRYER_RULES}\n\n\
         ## Mutator\n\n* Use the Atelier tool `mutator`.\n{MUTATOR_RULES}\n\n\
         `checker` runs crapper, dryer and verifier at once; `cleaner` adds the mutator.\n\n\
         {PRACTICE}"
    )
}

/// The commands: name, title, what they do.
const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "crapper",
        "CRAP gate",
        "Score every function by complexity and coverage, and bring each one under CRAP 10 by splitting it, testing it, or both.",
    ),
    (
        "dryer",
        "Duplicate gate",
        "Find the duplicate functions and eliminate the duplication that ranks 0.8 or above.",
    ),
    (
        "verifier",
        "Merge gate",
        "Find the functions a split left that only fragment the code, and merge them into their only caller.",
    ),
    (
        "mutator",
        "Mutation gate",
        "Mutation testing: cover the uncovered, kill the survivors, delete dead code.",
    ),
    (
        "checker",
        "Check the code",
        "crapper, dryer and verifier, and fix what they find until the three pass.",
    ),
    (
        "cleaner",
        "Clean the code",
        "crapper, dryer, verifier and mutator, and fix what they find until the four pass.",
    ),
    (
        "ub-mode",
        "Reduced principles",
        "Follow the reduced principles for every change: First Make it Work, Then Make it Right (with the quality gates), Follow Practice.",
    ),
];

/// The commands (MCP prompts).
pub fn prompts() -> Vec<Value> {
    COMMANDS
        .iter()
        .map(|(name, title, description)| {
            let arguments = if *name == "ub-mode" {
                json!([])
            } else {
                json!([{"name": "paths", "description": "Files, folders or fragments of paths to look at, separated by spaces (the whole project by default).", "required": false}])
            };
            json!({"name": name, "title": title, "description": description, "arguments": arguments})
        })
        .collect()
}

/// The gates a command runs, each with the rules for its findings.
fn rules(name: &str) -> Vec<(&'static str, &'static str)> {
    let crapper = ("crapper", CRAPPER_RULES);
    let verifier = ("verifier", VERIFIER_RULES);
    let dryer = ("dryer", DRYER_RULES);
    let mutator = ("mutator", MUTATOR_RULES);
    match name {
        "crapper" => vec![crapper],
        "dryer" => vec![dryer],
        "verifier" => vec![verifier],
        "mutator" => vec![mutator],
        "checker" => vec![crapper, verifier, dryer],
        _ => vec![crapper, verifier, dryer, mutator],
    }
}

pub fn prompt(name: &str, args: &Value) -> Option<String> {
    if name == "ub-mode" {
        return Some(ub_mode());
    }
    let (_, title, _) = COMMANDS.iter().find(|(n, _, _)| *n == name)?;
    let paths = args["paths"]
        .as_str()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    let on = paths.map_or(String::new(), |p| format!(" on “{p}”"));
    let with = paths.map_or(String::new(), |p| {
        format!(
            " with `paths` {}",
            json!(p.split_whitespace().collect::<Vec<_>>())
        )
    });
    let mut steps = String::new();
    for (gate, rules) in rules(name) {
        steps += &format!("\n### {gate}\n\n{rules}\n");
    }
    let after = match name {
        "crapper" => {
            "\nOnce no function is above 10, call `verifier`: a split must not leave pieces that only fragment the code.\n"
        }
        "mutator" => {
            "\nThe mutator runs the tests once per mutant: it can take long. If it says the tests fail on the unmutated code, fix the tests first.\n"
        }
        _ => "",
    };
    Some(format!(
        "{title}{on}.\n\n\
         1. Call the Atelier tool `{name}`{with}. Its report ends with the verdict of each gate.\n\
         2. Work through the findings, one small unit at a time, gate by gate in this order:\n{steps}{after}\n\
         3. After each unit, run the tests, then call `{name}` again; stop when every gate passes or when what is left is a finding you judge worth keeping (say why).\n\n\
         {RIGHT}\n\n\
         ## Keep a Log\n\n* Keep a log of every action you take and the reason you took it.\n* Report that log upon completion."
    ))
}
