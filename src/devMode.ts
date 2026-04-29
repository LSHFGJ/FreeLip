export function shouldShowDevControls(search: string, storedValue: string | null): boolean {
  const params = new URLSearchParams(search.startsWith("?") ? search : `?${search}`);
  return params.get("dev") === "1" || storedValue === "1";
}

export function readDevControlsEnabled(): boolean {
  let storedValue: string | null = null;
  try {
    storedValue = window.localStorage.getItem("freelip.devControls");
  } catch {
    storedValue = null;
  }
  return shouldShowDevControls(window.location.search, storedValue);
}
