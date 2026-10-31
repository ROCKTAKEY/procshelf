import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { parseScreen, screenHash, type Screen } from "./navigation";

export function useNavigation() {
  const [screen, setScreen] = useState(() => parseScreen(location.hash));
  const [mobile, setMobile] = useState(
    () => matchMedia("(max-width: 640px)").matches,
  );
  const positions = useRef(new Map<string, number>());
  const current = useRef("");
  const key = `${mobile ? "mobile" : "desktop"}:${screenHash(screen)}`;

  useEffect(() => {
    const media = matchMedia("(max-width: 640px)");
    const resize = () => {
      setMobile(media.matches);
    };
    const visit = () => {
      setScreen(parseScreen(location.hash));
    };
    const remember = () =>
      positions.current.set(current.current, window.scrollY);
    const previous = history.scrollRestoration;
    history.scrollRestoration = "manual";
    media.addEventListener("change", resize);
    window.addEventListener("popstate", visit);
    window.addEventListener("hashchange", visit);
    window.addEventListener("scroll", remember, { passive: true });
    return () => {
      history.scrollRestoration = previous;
      media.removeEventListener("change", resize);
      window.removeEventListener("popstate", visit);
      window.removeEventListener("hashchange", visit);
      window.removeEventListener("scroll", remember);
    };
  }, []);

  useLayoutEffect(() => {
    current.current = key;
    window.scrollTo(0, positions.current.get(key) ?? 0);
  }, [key]);

  function navigate(next: Screen, replace = false) {
    positions.current.set(current.current, window.scrollY);
    if (screenHash(next) === screenHash(screen)) return;
    const state = { returnTo: screenHash(screen) };
    if (replace) history.replaceState(null, "", screenHash(next));
    else history.pushState(state, "", screenHash(next));
    setScreen(next);
  }

  function back(next: Screen) {
    positions.current.set(current.current, window.scrollY);
    if (history.state?.returnTo === screenHash(next)) history.back();
    else navigate(next);
  }

  return { screen, mobile, navigate, back };
}
