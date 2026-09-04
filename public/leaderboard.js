// The house regulars play the same games people do, and they play far more of
// them, so they crowd every board. The checkbox hides them; the choice sticks,
// because whoever wants the boards without the house wants that every visit.
const KEY = "two-seven:leaderboard:show-house";
const board = document.getElementById("leaderboard");
const toggle = document.getElementById("show-house");

function apply(show) {
  board.classList.toggle("hide-house", !show);
}

if (board && toggle) {
  const stored = localStorage.getItem(KEY);
  toggle.checked = stored !== "off";
  apply(toggle.checked);
  toggle.addEventListener("change", () => {
    localStorage.setItem(KEY, toggle.checked ? "on" : "off");
    apply(toggle.checked);
  });
}
