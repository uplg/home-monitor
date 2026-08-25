/**
 * Haptic feedback for the on-screen remotes.
 *
 * Fires on touch, never on the response. The point is to confirm the press
 * landed under the finger, and an ADB round-trip to the TV box takes ~150 ms
 * — long enough that feedback arriving with the reply would read as lag
 * rather than as acknowledgement. Failures get their own pattern afterwards,
 * which is the one case worth waiting for.
 *
 * `navigator.vibrate` is Android-only; iOS Safari does not implement it, and
 * some embedded webviews throw outright. Every call is therefore guarded:
 * silence is a perfectly good outcome here, an exception is not.
 */

type Pattern = number | number[];

/** Light tick for navigation keys — the most repeated gesture. */
export const TAP: Pattern = 8;
/** Slightly firmer, for actions that commit to something (OK, launch, power). */
export const CONFIRM: Pattern = 16;
/** Distinguishable double buzz, so a failure is felt without looking. */
export const FAILURE: Pattern = [12, 40, 12];

export function haptic(pattern: Pattern = TAP): void {
  try {
    // `vibrate` is absent on iOS and on desktop; both are fine.
    navigator.vibrate?.(pattern);
  } catch {
    // Some webviews expose it and then refuse to run it. Nothing to do, and
    // nothing worth surfacing to the user over a missing buzz.
  }
}
