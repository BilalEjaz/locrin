import DOMPurify from "dompurify";
import sanitizeHtml from "sanitize-html";

export function Fixed() {
  return <div dangerouslySetInnerHTML={{ __html: "<p>hello</p>" }} />;
}

export function Cleaned(props: { html: string }) {
  return <div dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(props.html) }} />;
}

export function render(el: HTMLElement, body: string): void {
  el.innerHTML = sanitizeHtml(body);
}

export function label(el: HTMLElement, name: string): void {
  el.textContent = name;
}

export function escaped(el: HTMLElement, body: string): void {
  el.innerHTML = escapeHtml(body);
}
