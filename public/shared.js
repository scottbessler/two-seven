export function cents(value) {
  return Math.round(Number(value) * 100);
}

export function money(value, whole = false) {
  const sign = value < 0 ? "-" : "";
  const abs = Math.abs(value);
  if (whole) return `${sign}$${Math.round(abs / 100).toLocaleString()}`;
  return `${sign}$${Math.floor(abs / 100).toLocaleString()}.${String(abs % 100).padStart(2, "0")}`;
}

export async function responseError(response) {
  const text = await response.text();
  try {
    return JSON.parse(text).error || text;
  } catch {
    const document = new DOMParser().parseFromString(text, "text/html");
    return document.querySelector("p")?.textContent?.trim() || text || `Request failed (${response.status})`;
  }
}
