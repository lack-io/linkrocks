#![allow(dead_code)]

use raftpb::{
    new_conf_change_single,
    prelude::{ConfChangeSingle, ConfChangeType, ConfState},
};

use super::changer::Changer;
use crate::errors::Result;
use crate::tracker::ProgressTracker;

/// Translates a conf state into 1) a slice of operations creating first the config that
/// will become the outgoing one, and then the incoming one, and b) another slice that,
/// when applied to the config resulted from 1), represents the ConfState.
fn to_conf_change_single(cs: &ConfState) -> (Vec<ConfChangeSingle>, Vec<ConfChangeSingle>) {
    // Example to follow along this code:
    // voters=(1 2 3) learners=(5) outgoing=(1 2 4 6) learners_next=(4)
    //
    // This means that before entering the joint config, the configuration
    // had voters (1 2 4 6) and perhaps some learners that are already gone.
    // The new set of voters is (1 2 3), i.e. (1 2) were kept around, and (4 6)
    // are no longer voters; however 4 is poised to become a learner upon leaving
    // the joint state.
    // We can't tell whether 5 was a learner before entering the joint config,
    // but it doesn't matter (we'll pretend that it wasn't).
    //
    // The code below will construct
    // outgoing = add 1; add 2; add 4; add 6
    // incoming = remove 1; remove 2; remove 4; remove 6
    //            add 1;    add 2;    add 3;
    //            add-learner 5;
    //            add-learner 4;
    //
    // So, when starting with an empty config, after applying 'outgoing' we have
    //
    //   quorum=(1 2 4 6)
    //
    // From which we enter a joint state via 'incoming'
    //
    //   quorum=(1 2 3)&&(1 2 4 6) learners=(5) learners_next=(4)
    //
    // as desired.
    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();
    for id in &cs.voters_outgoing {
        outgoing.push(new_conf_change_single(
            *id,
            ConfChangeType::ConfChangeAddNode,
        ));
    }

    // We're done constructing the outgoing slice, now no to the incoming one
    // (which will apply on top of the config created by the outgoing slice).

    // First, we'll remove all of the outgoing voters.
    for id in &cs.voters_outgoing {
        incoming.push(new_conf_change_single(
            *id,
            ConfChangeType::ConfChangeRemoveNode,
        ));
    }

    // Then we'll add the incoming voters and learners.
    for id in &cs.voters {
        incoming.push(new_conf_change_single(
            *id,
            ConfChangeType::ConfChangeAddNode,
        ));
    }
    for id in &cs.learners {
        incoming.push(new_conf_change_single(
            *id,
            ConfChangeType::ConfChangeAddLearnerNode,
        ));
    }
    // Same for LearnersNext; these are nodes we want to be learners but which
    // are currently voters in the outgoing config.
    for id in &cs.learners_next {
        incoming.push(new_conf_change_single(
            *id,
            ConfChangeType::ConfChangeAddLearnerNode,
        ));
    }
    (outgoing, incoming)
}

/// Restore takes a Changer (which must represent an empty configuration), and runs a
/// sequence of changes enacting the configuration described in the ConfState.
///
/// TODO(jay) find a way to only take `ProgresMap` instend of a whole tracker.
pub fn restore(tracker: &mut ProgressTracker, next_idx: u64, cs: &ConfState) -> Result<()> {
    let (outgoing, incoming) = to_conf_change_single(cs);
    if outgoing.is_empty() {
        for i in incoming {
            let (cfg, changes) = Changer::new(tracker).simple(&[i])?;
            tracker.apply_conf(cfg, changes, next_idx);
        }
    } else {
        for cc in outgoing {
            let (cfg, changes) = Changer::new(tracker).simple(&[cc])?;
            tracker.apply_conf(cfg, changes, next_idx);
        }
        let (cfg, changes) = Changer::new(tracker).enter_joint(cs.auto_leave, &incoming)?;
        tracker.apply_conf(cfg, changes, next_idx);
    }

    Ok(())
}
