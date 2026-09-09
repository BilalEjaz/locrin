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

// A quoted key is still a name. The delimiter that closed the identifier hole
// asked for the separator immediately after the credential name, which dropped
// every JSON style key: a header object, an AWS profile written as JSON, a
// config map. The closing quote in front of the separator is optional, and the
// opening quote after it is not, so the identifier hole stays shut.
export const headers = { "Authorization": "Bearer Bd3Yh8Lc1Ws5Ej7Rn2Qz6Tp0" };
export const awsProfile = { "aws_secret_access_key": "zQ8mNv3XpLd6RtY1sCe4fHj7GkBw2ZaU5nMq9Tr0" };

// A legacy encrypted PEM. OpenSSL writes two headers between the BEGIN line and
// the base64 body, so the material check that reads the next source line found
// `Proc-Type:` there and called the whole block not a key.
export const legacyPem = `-----BEGIN RSA PRIVATE KEY-----
Proc-Type: 4,ENCRYPTED
DEK-Info: DES-EDE3-CBC,8F3A2B1C4D5E6F70

MIIEowIBAAKCAQEAsynthetic0third0key0for0locrin0only0Wm5Jt8Pq3Zr6
-----END RSA PRIVATE KEY-----`;
