// The decisions the rule makes past the shape of a match, kept out of keys.ts
// so that file stays exactly one line per entry in the table.

// A privileged role name is a credential wherever it sits.
export const superuserJwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJsb2NyaW4iLCJyb2xlIjoic3VwZXJ1c2VyIiwiaWF0IjoxNzAwMDAwMDAwLCJleHAiOjIwMDAwMDAwMDB9.SyntheticSignatureForLocrinFixturesOnly0000000";
// A role outside the privileged list is a credential because the line calls it
// a token. The clean fixture holds the same token under a name that does not.
export const deployBotTokenValue = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJsb2NyaW4iLCJyb2xlIjoidmlld2VyIiwiaWF0IjoxNzAwMDAwMDAwLCJleHAiOjIwMDAwMDAwMDB9.SyntheticSignatureForLocrinFixturesOnly0000000";

// Two private keys with different material. Anchored on the header they were
// one finding with one id, because the header is the same string everywhere.
export const signingPem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEAsynthetic0fixture0material0for0locrin0only0A1b2\n-----END RSA PRIVATE KEY-----";
// The same header with its material on the next source line, which is how a
// PEM written into a template literal reaches a repository. A different key,
// so a different finding with a different id.
export const deployPem = `-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEsynthetic0second0key0for0locrin0Zk4N
-----END PRIVATE KEY-----`;
