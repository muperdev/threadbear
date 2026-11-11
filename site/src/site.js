// The site's bits of motion and the home page's controls: the phone menu, the scroll reveals,
// the hero's player, the copy buttons, the (i) tips and the launch video. Everything here is an
// extra: without JavaScript the menu button stays hidden behind the desktop links on wide
// screens, every section is visible, the first clip shows as its poster and the prompt can be
// selected by hand.
const root = document.documentElement;
root.classList.add("js");

// The menu button opens a full height panel (styles.css does the movement).
const toggle = document.querySelector(".menu-toggle");
function setMenu(open) {
  root.classList.toggle("menu-open", open);
  toggle?.setAttribute("aria-expanded", String(open));
  toggle?.setAttribute("aria-label", open ? "Close the menu" : "Open the menu");
}
toggle?.addEventListener("click", () => setMenu(!root.classList.contains("menu-open")));
for (const link of document.querySelectorAll(".menu a")) link.addEventListener("click", () => setMenu(false));
// Widening past the phone layout (turning a big iPhone on its side, say) hides the menu and its
// button, so close it rather than leave it open where nobody can see it.
matchMedia("(max-width: 860px)").addEventListener("change", (event) => {
  if (!event.matches) setMenu(false);
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && root.classList.contains("menu-open")) {
    setMenu(false);
    toggle?.focus();
  }
});

// Sections rise in once as they come into view, through IntersectionObserver rather than a
// scroll listener.
const reveals = document.querySelectorAll(".reveal");
if ("IntersectionObserver" in window) {
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      entry.target.classList.add("in");
      observer.unobserve(entry.target);
    }
  }, { rootMargin: "0px 0px -10% 0px", threshold: 0.08 });
  reveals.forEach((el) => observer.observe(el));
} else {
  reveals.forEach((el) => el.classList.add("in"));
}

// The hero's player: each clip plays through, then the next one comes in, round and round. The
// features down the left jump about, and the playing one's bar is drawn every frame from its
// clip's currentTime, so it fills for the clip's real length and stops when the clip does. It
// holds while it's off screen, while the tab's hidden and after the pause button; clicking a
// feature plays it even after a pause, since that's asking to watch it. With reduced motion
// nothing plays by itself: the features change which clip's first frame shows.
const player = document.querySelector(".player");
if (player) {
  const clips = [...player.querySelectorAll(".slide")];
  const features = [...player.querySelectorAll(".feat")];
  const caption = player.querySelector(".player-caption");
  const toggleButton = player.querySelector(".player-toggle");
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let current = 0;
  let inView = true;
  let paused = still;
  // When autoplay's refused (Low Power Mode on an iPhone, say), the bar and the move to the next
  // clip run by the clock instead: when that clock started and how long the clip is.
  let clockStart = 0;
  let clockLength = 0;
  const held = () => paused || !inView || document.hidden;

  const setPaused = (on) => {
    paused = on;
    player.classList.toggle("paused", on);
    toggleButton.setAttribute("aria-label", on ? "Play the clips" : "Pause the clips");
    play();
  };
  const play = () => {
    const clip = clips[current];
    if (held()) {
      clip.pause();
      clockStart = 0;
      return;
    }
    clip.play().catch(() => {
      clockLength = (clip.duration || 12) * 1000;
      clockStart = performance.now() - (clip.currentTime || 0) * 1000;
    });
  };
  const show = (index) => {
    clips[current].pause();
    current = (index + clips.length) % clips.length;
    clips.forEach((clip, i) => {
      clip.classList.toggle("on", i === current);
      // aria-hidden="" doesn't hide anything, so set "true" or take it off.
      if (i === current) clip.removeAttribute("aria-hidden");
      else clip.setAttribute("aria-hidden", "true");
    });
    features.forEach((feature, i) => {
      feature.classList.toggle("on", i === current);
      const button = feature.querySelector("button");
      if (i === current) button.setAttribute("aria-current", "true");
      else button.removeAttribute("aria-current");
      feature.style.setProperty("--p", still && i === current ? "1" : "0");
    });
    caption.textContent = features[current].querySelector(".feat-text").textContent;
    const clip = clips[current];
    clip.preload = "auto";
    if (clip.readyState > 0) clip.currentTime = 0;
    // The next clip loads while this one plays, so it's ready when its turn comes.
    clips[(current + 1) % clips.length].preload = "auto";
    clockStart = 0;
    play();
  };
  clips.forEach((clip, i) => clip.addEventListener("ended", () => i === current && show(current + 1)));
  features.forEach((feature, i) => feature.querySelector("button").addEventListener("click", () => {
    if (!still) {
      paused = false;
      player.classList.remove("paused");
      toggleButton.setAttribute("aria-label", "Pause the clips");
    }
    show(i);
  }));
  toggleButton.addEventListener("click", () => setPaused(!paused));
  // Phones only (styles.css): the clip full screen, where its text is big enough to read.
  player.querySelector(".player-full").addEventListener("click", () => {
    const clip = clips[current];
    if (clip.webkitEnterFullscreen) clip.webkitEnterFullscreen();
    else clip.requestFullscreen?.();
  });
  if (!still) {
    const tick = () => {
      const clip = clips[current];
      let progress;
      if (clockStart) {
        progress = Math.min(1, (performance.now() - clockStart) / clockLength);
        if (progress >= 1) show(current + 1);
      } else {
        progress = clip.duration ? clip.currentTime / clip.duration : 0;
      }
      features[current].style.setProperty("--p", progress.toFixed(4));
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }
  if ("IntersectionObserver" in window) {
    new IntersectionObserver(([entry]) => { inView = entry.isIntersecting; play(); }, { threshold: 0.2 })
      .observe(player.querySelector(".stage"));
  }
  document.addEventListener("visibilitychange", play);
  if (still) player.classList.add("paused");
  show(0);
}

// The install prompt is printed once (#prompt). Every copy button copies it, then says where it
// goes. If the clipboard says no (an old browser, or a page not served over https), the printed
// prompt gets selected for copying by hand.
const prompt = document.getElementById("prompt");
if (prompt) {
  const text = prompt.textContent.trim();
  for (const button of document.querySelectorAll(".copy-prompt")) {
    const label = button.querySelector("span");
    let reset = 0;
    button.addEventListener("click", async () => {
      let copied = false;
      try {
        await navigator.clipboard.writeText(text);
        copied = true;
      } catch {
        prompt.scrollIntoView({ block: "center" });
        getSelection().selectAllChildren(prompt);
      }
      button.classList.toggle("copied", copied);
      label.textContent = copied ? "Now paste it into Claude Code or Codex" : "It's selected, so press ⌘C";
      clearTimeout(reset);
      reset = setTimeout(() => {
        button.classList.remove("copied");
        label.textContent = "Copy the install prompt";
      }, 6000);
    });
  }

  // "See the prompt" in the hero: a link down to it without JavaScript, a peek at it here with.
  for (const link of document.querySelectorAll("[data-peek]")) {
    const peek = document.getElementById(link.getAttribute("aria-controls"));
    peek.querySelector("code").textContent = text;
    link.setAttribute("role", "button");
    link.addEventListener("click", (event) => {
      event.preventDefault();
      const open = link.getAttribute("aria-expanded") !== "true";
      link.setAttribute("aria-expanded", String(open));
      peek.hidden = !open;
      link.textContent = open ? "Hide the prompt" : "See the prompt";
    });
  }
}

// "Send this page to my Mac": only on a phone (or anything with a touch screen and no mouse)
// that has a share sheet, for AirDrop or Messages. A Mac's own share sheet would be no use here.
const sendToMac = document.querySelector(".send-mac");
if (sendToMac && navigator.share && matchMedia("(pointer: coarse)").matches) {
  sendToMac.hidden = false;
  sendToMac.addEventListener("click", () => {
    navigator.share({ title: "Threadbear", url: `${location.origin}/` }).catch(() => {});
  });
}

// The (i) tips: one open at a time, closed by a click anywhere else or Escape.
const tipButtons = [...document.querySelectorAll(".tip-btn")];
const closeTips = () => {
  for (const button of tipButtons) {
    button.setAttribute("aria-expanded", "false");
    document.getElementById(button.getAttribute("aria-controls")).hidden = true;
  }
};
for (const button of tipButtons) {
  button.addEventListener("click", (event) => {
    event.stopPropagation();
    const open = button.getAttribute("aria-expanded") !== "true";
    closeTips();
    if (!open) return;
    button.setAttribute("aria-expanded", "true");
    document.getElementById(button.getAttribute("aria-controls")).hidden = false;
  });
}
if (tipButtons.length) {
  document.addEventListener("click", closeTips);
  document.addEventListener("keydown", (event) => { if (event.key === "Escape") closeTips(); });
}

// The launch video (once LAUNCH_VIDEO_ID is set in site.mjs): the card links to YouTube, and a
// click here swaps it for the no-cookie player instead, so nothing from YouTube loads until then.
for (const card of document.querySelectorAll(".video-card[data-video]")) {
  card.addEventListener("click", (event) => {
    event.preventDefault();
    const frame = document.createElement("iframe");
    frame.src = `https://www.youtube-nocookie.com/embed/${card.dataset.video}?autoplay=1`;
    frame.title = "Threadbear launch video";
    frame.allow = "autoplay; encrypted-media; picture-in-picture; fullscreen";
    frame.allowFullscreen = true;
    // A div rather than the link, since a player inside a link would be a link too.
    const box = document.createElement("div");
    box.className = "video-card playing";
    box.append(frame);
    card.replaceWith(box);
  });
}

// The changelog (/changelog). A before/after: the slider moves where the before picture is cut
// off (a CSS variable, set through the CSSOM, which the CSP allows). Without this it stays
// halfway, so both halves still show. A clip only plays while it's on screen, and not at all
// for someone who's asked for less motion (its poster stays).
for (const box of document.querySelectorAll(".cl-compare")) {
  const slider = box.querySelector(".cl-slider");
  const set = () => box.style.setProperty("--cut", `${slider.value}%`);
  slider.addEventListener("input", set);
  set();
}
const clips = document.querySelectorAll("video.cl-shot");
if (clips.length && "IntersectionObserver" in window && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
  const watcher = new IntersectionObserver((entries) => {
    for (const { target, isIntersecting } of entries) {
      if (isIntersecting) target.play().catch(() => {});
      else target.pause();
    }
  }, { threshold: 0.4 });
  for (const clip of clips) watcher.observe(clip);
}
