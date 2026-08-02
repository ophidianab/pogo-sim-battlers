Feb 17, 2026
# Strengthening the Foundations of Pokémon GO Trainer Battles

The new PvP battle system is now available to all Trainers. Please see more details here.

Trainers,

From the beginning, Pokémon GO Trainer Battles were designed under two principles. The gameplay had to be fast paced for the mobile landscape so that players could have short experiences while exploring the world, and battles needed to be dynamic enough to invoke a sense of skill when managing your Pokémon. Given the same game state and decisions, outcomes should be driven by player choices, not game state quirks, device differences, or momentary network hiccups.

As the player versus player (PvP) community has grown more competitive, players have naturally pushed the system to its limits. In doing so, they may have noticed situations where battles did not always behave as consistently as expected. These moments were rare, but at higher levels of play, even small inconsistencies could have an outsized impact.

Rather than continuing to address these issues one at a time, the team took a broader approach by rebuilding the core battle system. The goal was to harden the foundations of all battle modes, including PvP, so that battles consistently reflect player skill while maintaining the fast, responsive feel players expect. With a systemic change like this, players can expect to see changes. To help find a balance between addressing existing consistency issues and game feel, we’ve solicited feedback during development from the community as well as various players from both the grassroots competitive scene and the official Pokémon GO Championship Series.

## Supporting Skill-Based Battles

Battles in Pokémon GO have very short, fixed-duration turns. This structure helps maintain a consistent battle state while delivering smooth and responsive gameplay.

In the past, some interactions could be influenced by when inputs arrived within those turns. Most of the time this worked as intended, but differences in network conditions or device performance could occasionally affect how actions were resolved.

The updated battle system reinforces turn-based resolution at a foundational level, better mitigating the impact of network hiccups and device performance differences. While connectivity will always play a role in online play, normal variations within turn and network tolerances should no longer change the outcome of identical decisions for a given game state.

## Major Changes
### Damage Resolves at the End of the Turn

Previously, damage could resolve at different points within a turn, depending on network conditions. With Fast Attacks that take one turn, this sometimes meant one player’s move was resolved slightly earlier than the other player’s under certain conditions.

New expected behavior:

    Damage resolves at the end of each turn.
    Energy is awarded at the same time damage is resolved.
    Charged Attacks will now consistently resolve if used on the same turn the user is about to faint from a Fast Attack.

What this means for players:

    One-turn Fast Attacks now reliably tie when used on the same turn.
    Energy gains are predictable and easier to track.
    Outcomes depend on decisions and timing within the turn system, not a player’s local network speed or variance due to provider differences.

### Swaps Resolve Before Damage

Another area players may notice changes is around swapping Pokémon to absorb an incoming attack.

Previously, whether damage landed on the outgoing or incoming Pokémon could depend on timing of the swap as well as connection quality.

New expected behavior:

    Swaps always resolve before damage.

What this means for players:

    When you swap on the same turn as an opponent’s Fast Attack completes, the incoming Pokémon will consistently take the damage.
    When swapping on the same turn that your opponent’s Charged Attack starts, the incoming Pokemon will be the recipient of the Charged Attack.
    Swapping to absorb incoming damage will behave the same way across different network conditions within the turn.
    Swap decisions are more reliable and easier to plan around.

### Clear and Consistent Swap Costs

Swap timing has also been standardized.

A quick swap is a voluntary swap made during normal battle flow. A forced swap happens when a Pokémon faints and a replacement must be chosen.

New expected behavior:

    Quick swaps always cost one turn.
    Forced swaps do not cost a turn.
    Swaps during the end of the Charged Attack sequence will not cost a turn.

What this means for players:

    Swap timing is clear and predictable.
    Inconsistent delays around Charged Attacks and swaps have been removed.
    Planning around swaps is more intuitive, especially in tight endgame scenarios.
    Input delays due to swap reliability are eliminated.

### Charged Attacks Start on the Next Turn

Charged Attack timing has been standardized so that Charged Attacks always begin at the start of the following turn after being triggered.

Previously, Charged Attacks could begin immediately, which created uneven timing windows and made some interactions more sensitive to latency or device performance.

New expected behavior:

    Charged Attacks begin at the start of the next turn.
    Charged Attack damage and effects will always resolve before Fast Attacks that finish during the Charged Attack sequence.
    This means that stat changes, as well as effects that alter a Pokémon, will occur before any incoming Fast Attack effects are applied.

What this means for players:

    Timing windows around Charged Attacks are more consistent.
    Fast Attack “Sneak”
        Fast Attacks started at the beginning of an opponent’s Charged Attack rely less on connection quality or device speed.
        Fast Attacks started at the beginning of an opponent’s Charged Attack can only occur on the exact turn a Charged Attack is used.
    Fast Attack “Leak”
        Fast Attacks that finish during an opponent’s Charged Attack will consistently be applied after the Charged Attack is completed. This means that Charged Attacks that cause stat changes will be applied before the Fast Attack is applied.
    Charged Attack outcomes are more consistent, barring chance-based effects such as stat changes from attacks like Night Slash.

### Earlier Swap UI During Charged Attacks

The swap flyout now appears slightly earlier as a Charged Attack sequence is ending.

What this means for players:

    It is easier to queue a swap as a Charged Attack finishes.
    Inputs are less likely to be missed during fast-paced moments.
    Visual feedback better matches when the system is ready for your next action.

### Battles Continue Even If a Trainer Disconnects

Previously, battles depended heavily on continuous communication from both players. If a player disconnected or stopped sending input, game state and outcomes could become unclear.

This was especially noticeable in situations where both players were on the brink of a knockout and order mattered, especially for setting up energy or alignment on the incoming Pokémon.

New expected behavior:

    Both players must connect before a battle begins.
    Battles continue to progress even if a client stops responding.
    Battles will always continue to provide visual feedback if either player stops tapping.

What this means for players:

    Battles resolve cleanly if an opponent disconnects or leaves the match.
    Outcomes around knockouts, energy gain, and alignment are clear and consistent.
    Delays due to player input during a Charged Attack will no longer impact game pacing and flow.
    Situations where both players do not want to submit actions are now more clear.

### HP Bar Animation Updates

With damage resolving at the end of turns, HP bar behavior has been updated to better communicate when damage is pending versus when it is finalized.

In rare Charged Attack timing scenarios, players may briefly see pending damage before resolution. Rest assured that the HP bar animations resolve only when damage is applied.

## What This Means for Competitive Play

Together, these updates strengthen the competitive experience, improving reliability and fairness for all Trainers and leading to the following.

    Consistent outcomes for identical game states and decisions.
    Reduced sensitivity to latency and device performance.
    Clearer and more reliable timing for advanced techniques.

For experienced players, this means cleaner timing, more dependable swap interactions with Charged Attacks, damage registering predictably, and fewer moments where outcomes feel unexpected. For all players, it means battles that more accurately reflect skillful play.

## Looking Ahead

This update focuses on reinforcing the core battle system so future updates across all battle modes can build on a stable and consistent foundation.

We will continue monitoring battles as these changes roll out, adjusting as necessary. We appreciate the community’s patience and continued passion for the GO Battle League.

We will see you in GBL! Just maybe take it easier on my Shadow Dragonite…

Please be aware of your surroundings and follow guidelines from local health authorities when playing Pokémon GO. Upcoming events are subject to change. Be sure to follow us on social media, opt in to receiving push notifications, and subscribe to our emails to stay updated.

—The Pokémon GO team
