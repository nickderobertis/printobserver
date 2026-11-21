"""An outside supervisor's conversation, replayed beside printobserver's own agent.

A person's or an operator's agent that started a print follows it with `printobserver
watch`, and is there to catch what printobserver's agent misses. So at each of the
print's milestones a case names — its first layer, the first layer that prints an
overhang, the first that prints support, and its end — the replay hands that agent
the watch's latest line, which says the milestone and printobserver's agent's latest
verdict, "still clean", as the next message of one `oneharness run --session`
conversation, on the shipped harness with the skill installed as a person installs an
Agent Skill.

The line is handed while printobserver's turn before it still holds the print, so the
job stands at the milestone and the camera answers the frame nearest after it. It is
judged from the server's record: a look the supervisor took of its own at the
printer, with no print named, whose stored frame it opened; no look naming the print;
and nothing asked of the print that changes what the machine is doing.
"""

from __future__ import annotations

from typing import Any

from cases import Milestone, Moment, Request
from replay import REQUESTED, OutsidePoint, PrintId, Replay, neutral_environment
from starts import OPERATOR, OperatorAgent, SessionName

#: What the watch's line says the print has reached, by milestone.
MILESTONE_WORDS = {
    Milestone.FIRST_LAYER: "its first layer",
    Milestone.FIRST_OVERHANG: "the first layer that prints an overhang",
    Milestone.FIRST_SUPPORT: "the first layer that prints support",
    Milestone.END: "its last layer",
}
#: What the watch's line says printobserver's agent last decided.
LATEST_VERDICT = "still clean"
#: A conversation's name: this and the case's name.
SESSION_PREFIX = "outside-"


def watch_line(moment: Moment, print_id: PrintId) -> str:
    """The one message a milestone is: the print, the watch's latest line, and who acts."""
    if moment.milestone is None:
        msg = f"{moment.id} names no milestone for the watch's line to report"
        raise ValueError(msg)
    return (
        f"You started the print {print_id} and follow it with printobserver watch. "
        f"The watch's latest line: the print has reached {MILESTONE_WORDS[moment.milestone]}; "
        f"printobserver's agent's latest verdict: {LATEST_VERDICT}. "
        f"You act for the operator: your actor is {OPERATOR}."
    )


class Outside:
    """The outside supervisor of one replay: its own home and harness, and one conversation."""

    def __init__(self, replay: Replay, target: list[str]) -> None:
        """Lay its home out beside the replay's install, the skill installed in it."""
        self.replay = replay
        root = replay.root / "outside"
        self.session = SessionName(f"{SESSION_PREFIX}{replay.case.name}")
        self.agent = OperatorAgent(
            home=root / "home",
            agent_bin=root / "agent",
            records=root / "records",
            conversations=root / "conversations",
            target=target,
            environment=self._environment,
            failed=replay.errors.append,
        )
        self.agent.install()

    def _environment(self) -> dict[str, str]:
        """The operator's own: where the server is, their credential, and their harness's home."""
        return {
            **neutral_environment(self.replay.bin, self.agent.home),
            "PRINTOBSERVER_SERVER": f"http://127.0.0.1:{self.replay.port}",
            "PRINTOBSERVER_CREDENTIAL": self.replay.minted.operator,
            # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
            "CLAUDE_CONFIG_DIR": str(self.agent.config_dir),
        }

    def _stored(self) -> set[str]:
        """The name of every frame and image the server has stored."""
        return {path.name for path in (self.replay.state / "images").glob("*/*") if path.is_file()}

    def _requests(self) -> list[Request]:
        """Every action the operator asked of the print, oldest first."""
        found = []
        for event in self.replay.history():
            if event["kind"] not in REQUESTED:
                continue
            action = event["payload"]["action"]
            if action.get("actor") == OPERATOR:
                operation = str(action["action"]).replace("_", "-")
                found.append(Request(operation, dict(action)))
        return found

    def supervise(self, moment: Moment, print_id: PrintId) -> OutsidePoint:
        """Hand it the watch's line at one milestone, and judge it from the server's record."""
        stored = self._stored()
        fetched = len(self.replay.timeline.fetches)
        asked = len(self._requests())
        conversed = self.agent.converse(self.session, watch_line(moment, print_id))
        return OutsidePoint(
            moment=moment,
            session=self.session,
            phase=conversed.phase,
            report=conversed.report,
            reply=conversed.reply,
            failure=conversed.failure,
            requests=self._requests()[asked:],
            stored=sorted(self._stored() - stored),
            looks=len(self.replay.timeline.fetches) - fetched,
        )

    def transcript(self) -> list[dict[str, Any]]:
        """Everything its harness runs wrote."""
        return self.agent.transcript()

    def stop(self) -> None:
        """Stop its control double."""
        self.agent.stop()
