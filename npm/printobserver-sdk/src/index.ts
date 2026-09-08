/**
 * Typed Node client for the printobserver server's HTTP surface.
 *
 * This module is deliberately empty of client code: the baseline node stands up
 * the toolchain and the gate targets, and the `sdks` node adds the client
 * itself plus the distribution that publishes it.
 */

/** The surfaces this client will expose once the `sdks` node writes them. */
export type ClientSurface = never;

/** Names of the exports this package ships. Empty until the `sdks` node lands. */
export const exportedNames: readonly string[] = [];
