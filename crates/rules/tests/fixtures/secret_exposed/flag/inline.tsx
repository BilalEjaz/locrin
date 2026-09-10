// The structural tokens the placeholder gate used to read over the whole line:
// an angle bracket, a `${}` interpolation and a `process.env` reference. Every
// line below holds a committed credential with one of them beside it, and every
// one of them is a finding.

// A tag is not a placeholder for the key in its own props.
export const MapPanel = <GoogleMap apiKey="AIzaSyD3f7Hq2Kp9Lm4Nv8Rt6Bw1Xc5Zy0Ae2Qk" />;

// Neither is a generic parameter.
export function Checkout() {
    const [stripeKey] = useState<string>("sk_live_A1b2C3d4E5f6G7h8I9j0K1l2M3n4");
    return stripeKey;
}

// A template literal that interpolates something else still carries the key.
export const tiles = (q: string) => `https://tiles.internal/v1?key=AIzaSyD3f7Hq2Kp9Lm4Nv8Rt6Bw1Xc5Zy0Ae2Qk&q=${q}`;

// A fallback beside an environment reference is the value that ships when the
// variable is unset, which is the whole reason it was written down.
export const mapsKey = process.env.MAPS_KEY || "AIzaSyD3f7Hq2Kp9Lm4Nv8Rt6Bw1Xc5Zy0Ae2Qk";

declare function useState<T>(initial: T): [T, (next: T) => void];
declare const GoogleMap: (props: { apiKey: string }) => unknown;
declare const process: { env: Record<string, string> };
