// The must-not-flag half. Every provider in the table has a line here: the
// environment reference a real repository writes, the placeholder its
// documentation ships, the public identifier that is meant to be in the bundle,
// or the near miss that is one character short of the shape.

// Environment references. Nothing here is a value.
export const awsAccessKeyId = process.env.AWS_ACCESS_KEY_ID;
export const awsSecretAccessKey = process.env.AWS_SECRET_ACCESS_KEY;
export const awsSessionToken = process.env.AWS_SESSION_TOKEN;
export const googleMapsKey = process.env.GOOGLE_MAPS_API_KEY;
export const googleOauthAccess = process.env.GOOGLE_OAUTH_ACCESS_TOKEN;
export const googleOauthClientSecret = process.env.GOOGLE_OAUTH_CLIENT_SECRET;
export const serviceAccountJson = process.env.GCP_SERVICE_ACCOUNT_JSON;
export const fcmServerKey = process.env.FCM_SERVER_KEY;
export const azureBlobConnection = process.env.AZURE_STORAGE_CONNECTION_STRING;
export const azureClientSecret = process.env.AZURE_CLIENT_SECRET;
export const azureDevopsPat = process.env.AZURE_DEVOPS_PAT;
export const alibabaKeyId = process.env.ALIBABA_ACCESS_KEY_ID;
export const alibabaAccessKeySecret = process.env.ALIBABA_ACCESS_KEY_SECRET;
export const tencentSecretId = process.env.TENCENT_SECRET_ID;
export const yandexCloudApiKey = process.env.YANDEX_CLOUD_API_KEY;
export const ibmCloudApiKey = process.env.IBM_CLOUD_API_KEY;
export const githubToken = process.env.GITHUB_TOKEN;
export const githubFineGrained = process.env.GITHUB_FINE_GRAINED_TOKEN;
export const gitlabToken = process.env.GITLAB_TOKEN;
export const gitlabTriggerToken = process.env.GITLAB_TRIGGER_TOKEN;
export const gitlabRunnerToken = process.env.GITLAB_RUNNER_TOKEN;
export const gitlabDeployToken = process.env.GITLAB_DEPLOY_TOKEN;

// Documented placeholders, the shape without the value.
export const bitbucketAppPassword = "your-bitbucket-app-password";
export const circleCiToken = "changeme0000000000000000000000000000changeme";
export const buildkiteAgentToken = "bkua_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
export const travisToken = "your_travis_token_value";
export const snykApiToken = "00000000-0000-0000-0000-000000000000";
export const postmanApiKey = "PMAK-example000000000000000-example00000000000000000000000000000000";
export const pulumiAccessToken = "pul-0000000000000000000000000000000000000000";
export const dopplerServiceToken = "dp.pt.your_doppler_service_token_goes_here00";
export const vaultToken = "hvs.example_vault_token_value";
export const terraformCloudToken = "exampleexample.atlasv1.exampleexampleexampleexampleexampleexampleexample";
export const onePasswordToken = "ops_your_service_account_token_here_00000000";

// Public identifiers. Every one of these is meant to reach a browser.
export const stripePublishableKey = "pk_live_A1b2C3d4E5f6G7h8I9j0K1l2M3n4";
export const stripeTestKey = "sk_test_A1b2C3d4E5f6G7h8I9j0K1l2M3n4";
export const mapboxPublicToken = "pk.eyJ1IjoibG9jcmluIiwiYSI6ImNrMTIzNDU2Nzg5MCJ9.A1b2C3d4E5f6G7h8I9j0K1";
export const sentryPublicDsn = "https://9f2c1a7b4e6d8039acbe5172d0f34a6b@o12345.ingest.sentry.io/1234567";
export const twilioAccountSid = "AC9f2c1a7b4e6d8039acbe5172d0f34a6b";
export const SUPABASE_ANON_KEY = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoiYW5vbiIsImlhdCI6MTcwMDAwMDAwMCwiZXhwIjoyMDAwMDAwMDAwfQ.SyntheticSignatureForLocrinFixturesOnly0000000";
export const userSessionToken = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoiYXV0aGVudGljYXRlZCIsImlhdCI6MTcwMDAwMDAwMCwiZXhwIjoyMDAwMDAwMDAwfQ.SyntheticSignatureForLocrinFixturesOnly0000000";
// An application role that is not one of the privileged names. It is a user's
// session, not a credential, and the line does not say otherwise.
export const viewerSession = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJsb2NyaW4iLCJyb2xlIjoidmlld2VyIiwiaWF0IjoxNzAwMDAwMDAwLCJleHAiOjIwMDAwMDAwMDB9.SyntheticSignatureForLocrinFixturesOnly0000000";
// A service role key whose `exp` is in the past. It is not live, so there is
// nothing to revoke and nothing to block a build over.
export const rotatedServiceRoleKey = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoic2VydmljZV9yb2xlIiwiaWF0IjoxNjAwMDAwMDAwLCJleHAiOjE2MDAwMDM2MDB9.SyntheticSignatureForLocrinFixturesOnly0000000";

// Development connection strings: a local host, a password equal to its
// username, or one of the words every compose file uses.
export const localPg = "postgresql://postgres:postgres@localhost:54322/postgres";
export const localMongo = "mongodb://root:root@localhost:27017/app";
export const localMysql = "mysql://appuser:password@127.0.0.1:3306/app";
export const localRedis = "redis://default:redis@localhost:6379";
export const localAmqp = "amqp://guest:guest@localhost:5672";
export const dockerPg = "postgres://appuser:secret@host.docker.internal:5432/app";

// Slack, chat and social: placeholders and near misses.
export const slackBotToken = "xoxb-your-bot-token";
export const slackAppToken = "xapp-1-placeholder";
export const slackWebhook = "https://hooks.slack.com/services/YOUR/WEBHOOK/URL";
export const discordBotToken = "your.discord.token";
export const discordWebhook = "https://discord.com/api/webhooks/000000000000000000/your-webhook-token";
export const telegramBotToken = "123:AA-placeholder";
export const twitterBearer = "AAAA-your-bearer-token";
export const twitterApiSecret = "your_twitter_api_secret_value_here";
export const facebookAccessToken = "EAA-your-access-token";
export const fbAppSecret = "your_fb_app_secret";
export const instagramToken = "IGQVJ-your-instagram-token";
export const linkedinClientSecret = "your_linkedin_secret";

// Mail and support.
export const sendgridApiKey = "SG.your-sendgrid-key";
export const mailgunApiKey = "key-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
export const mailchimpApiKey = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx-us1";
export const postmarkServerToken = "00000000-0000-0000-0000-000000000000";
export const resendApiKey = "re_your_api_key";
export const zendeskApiToken = "your_zendesk_api_token";
export const freshdeskApiKey = "your_freshdesk_key";
export const intercomAccessToken = "dG9r-your-intercom-token";
export const twilioAuthToken = "your_twilio_auth_token";

// Registries.
export const npmToken = "npm_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
export const pypiToken = "pypi-your-token";
export const rubygemsKey = "rubygems_your_key";
export const nugetKey = "oy2-your-nuget-key";
export const dockerHubToken = "dckr_pat_your-token";
export const cratesIoToken = "cio-your-token";
export const jfrogKey = "AKCp-your-artifactory-key";

// Hosting.
export const herokuApiKey = "00000000-0000-0000-0000-000000000000";
export const digitalOceanToken = "dop_v1_your_token";
export const linodeApiToken = "your_linode_token";
export const vultrApiKey = "YOUR-VULTR-API-KEY";
export const hetznerApiToken = "your_hetzner_token";
export const cloudflareApiToken = "your-cloudflare-api-token";
export const cloudflareGlobalApiKey = "your-cloudflare-global-key";
export const vercelToken = "your_vercel_token";
export const netlifyToken = "nfp_your_netlify_token";
export const railwayApiToken = "00000000-0000-0000-0000-000000000000";
export const renderApiKey = "rnd_your_render_key";
export const flyioToken = "fo1_your_fly_token";
export const expoToken = "your_expo_token";

// Databases.
export const planetscalePassword = "pscale_pw_your_password";
export const planetscaleToken = "pscale_tkn_your_token";
export const neonApiKey = "your_neon_api_key";
export const upstashRedisRestToken = "your_upstash_rest_token";
export const supabaseAccessToken = "sbp_your_personal_access_token";
// Supabase's newer publishable key, the half a browser is meant to hold. The
// secret half is spelled `sb_secret_`, and that is the one the table matches.
export const supabasePublishableKey = "sb_publishable_A1b2C3d4E5f6G7h8I9j0K1l2";

// Product and observability.
export const airtableToken = "pat-your-airtable-token";
export const notionToken = "secret_your_notion_integration_token";
export const linearApiKey = "lin_api_your_linear_key";
export const asanaToken = "1/0000000000000000:your-asana-token";
export const atlassianToken = "ATATT3-your-atlassian-token";
export const jiraApiToken = "your_jira_api_token";
export const sentryAuthToken = "sntrys_your_sentry_auth_token";
export const datadogAgentToken = "ddapi_your_datadog_token";
export const datadogApiKey = "your_datadog_api_key";
export const newRelicUserKey = "NRAK-YOUR-NEW-RELIC-USER-KEY";
export const newRelicLicenseKey = "your-new-relic-license-key";
export const grafanaToken = "glsa_your_grafana_token";
export const pagerdutyApiKey = "your_pagerduty_api_key";
export const opsgenieApiKey = "00000000-0000-0000-0000-000000000000";
export const rollbarAccessToken = "your_rollbar_token";
export const bugsnagApiKey = "your_bugsnag_api_key";
export const mezmoIngestionKey = "your_mezmo_key";
export const segmentWriteKey = "your_segment_write_key";
export const amplitudeSecretKey = "your_amplitude_secret_key";
export const mixpanelApiSecret = "your_mixpanel_secret";
export const algoliaAdminApiKey = "your_algolia_admin_key";
export const launchDarklySdkKey = "sdk-00000000-0000-0000-0000-000000000000";
export const statsigSecretKey = "secret-your-statsig-key";
// The Branch Key: the public half, in every mobile bundle Branch ships in. The
// secret half is spelled `secret_live_`, and that is the one the table matches.
export const branchLiveKey = "key_live_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6";
export const onesignalRestApiKey = "your_onesignal_rest_api_key";
export const appsflyerDevKey = "your_appsflyer_dev_key";
// RevenueCat's public SDK keys, which every client is configured with and which
// are `appl_` and `goog_`. The secret key is `sk_`.
export const revenuecatSecretKey = "appl_A1b2C3d4E5f6G7h8I9j0K1l2";
export const revenuecatGooglePlaySecretKey = "goog_A1b2C3d4E5f6G7h8I9j0K1l2";
export const elasticApiKey = "your_elastic_api_key";

// Commerce.
export const stripeWebhookSecret = "whsec_your_webhook_secret";
export const shopifyAccessToken = "shpat_your_shopify_token";
export const squareAccessToken = "sq0atp-your-square-token";
export const squareOauthSecret = "sq0csp-your-square-secret";
export const paypalClientSecret = "your_paypal_client_secret";
export const braintreePrivateKey = "your_braintree_private_key";
export const adyenApiKey = "AQE-your-adyen-key";
export const plaidAccessToken = "access-sandbox-00000000-0000-0000-0000-000000000000";
export const plaidSecret = "your_plaid_secret";
export const coinbaseApiSecret = "your_coinbase_api_secret";
export const krakenPrivateKey = "your_kraken_private_key";
export const binanceApiSecret = "your_binance_secret";

// Identity.
export const oktaApiToken = "00-your-okta-token";
export const auth0ClientSecret = "your_auth0_client_secret";
export const salesforceClientSecret = "your_salesforce_client_secret";
export const zoomApiSecret = "your_zoom_api_secret";

// Content and storage.
export const dropboxAccessToken = "sl.your-dropbox-token";
export const figmaToken = "figd_your_figma_token";
export const contentfulManagementToken = "CFPAT-your-contentful-token";
export const sanityToken = "your_sanity_token";
export const storyblokManagementToken = "your_storyblok_token";
export const cloudinaryUrl = "cloudinary://your-key:your-secret@your-cloud";
export const pusherAppSecret = "your_pusher_secret";
export const streamApiSecret = "your_stream_api_secret";
export const hubspotPrivateAppToken = "pat-na1-00000000-0000-0000-0000-000000000000";
export const trelloApiSecret = "your_trello_secret";

// Model providers.
export const openaiApiKey = "sk-your-openai-key";
export const anthropicApiKey = "sk-ant-your-anthropic-key";
export const huggingFaceToken = "hf_your_hugging_face_token";
export const replicateToken = "r8_your_replicate_token";
export const groqApiKey = "gsk_your_groq_key";
export const perplexityApiKey = "pplx-your-perplexity-key";
export const cohereApiKey = "your_cohere_api_key";
export const mistralApiKey = "your_mistral_api_key";

// A structural token inside the value: an interpolation where the password
// goes, an angle bracket placeholder in a documented URI, an environment
// reference built into a template. None of these is a credential.
export const templateUri = `postgresql://appuser:${dbPassword}@db.prod.internal:5432/app`;
export const documentedUri = "mongodb+srv://appuser:<password>@cluster0.abcde.mongodb.net/app";
export const envUri = `postgresql://${process.env.PGUSER}:${process.env.PGPASSWORD}@db.prod.internal:5432/app`;

// Key material and headers built at runtime rather than written down.
export const publicKeyBlock = "-----BEGIN PUBLIC KEY-----";
// A header with no key material after it: a string a program strips out or
// compares against. There is no key here, and the header is the same string in
// every repository, so a finding anchored on it would be one id for every key.
export const stripped = pem.replace("-----BEGIN PRIVATE KEY-----", "");
export const looksLikeAKey = key.startsWith("-----BEGIN RSA PRIVATE KEY-----");
export const basicAuthHeader = `Basic ${btoa(user + ":" + secret)}`;
export const bearerHeader = { Authorization: `Bearer ${accessToken}` };
// A header holding a test double rather than a token, which the header shape
// cannot tell apart on its own.
export const mockHeaders = { Authorization: "Bearer mockAccessTokenForTheSuite" };
// And a header holding a provider sandbox key, which says so in the value.
export const sandboxHeaders = { Authorization: "Bearer sk_test_A1b2C3d4E5f6G7h8I9j0K1l2M3n4" };

// Written values, not generated ones: the entropy gate.
export const password = "password1234";
export const apiKey = "aaaaaaaaaaaaaaaaaaaaaaaa";
export const clientSecret = "abcabcabcabcabcabcabcabc";
export const uploadPath = "src/assets/images/hero-banner.png";
export const testPassword = "correcthorsebatterystaple";
// A form label, from the corpus: four character classes and over four bits per
// character, and still a sentence rather than a credential.
export const oauth2Password = "OAuth2 password grant";
// Sequential filler: three character classes, twenty three characters and
// 4.14 bits per character, and typed by a person all the same.
export const stubApiKey = "test_api_key_1234567890";
// And a value that says what it stands in for.
export const mockClientSecret = "mockSecretZk4Np7Qm2Rt9Vx6";

// A credential name beside an identifier rather than a literal. Every line
// below names a secret and holds none: the value is fetched, read off a config
// object, or is a type. A context entry that accepts an unquoted identifier
// turns every one of these into a locked High finding on a build nobody can
// unblock.
export const expoToken = getExpoTokenFromSecureStore();
// The same thing behind a quoted key. The closing quote in front of the
// separator is optional; the opening quote after it is not, so a JSON style
// name holding an identifier is still not a credential.
export const secretsFromStore = { "expoToken": getExpoTokenFromSecureStore() };
export const azureClientSecret = config.azure.credentials.clientSecret;
export const pagerdutyApiKey = pagerDutyIntegrationKey;
export const bitbucketAppPassword = credentials.bitbucketAppPassword;
export const revenuecatSecretKey = purchasesConfiguration.revenueCatSecretKey;

export interface SecretsShape {
    expoToken: ExpoAccessTokenConfiguration;
    pagerdutyApiKey: PagerDutyIntegrationKeyReference;
    linkedinClientSecret: LinkedInClientSecretReference;
}

declare const user: string;
declare const secret: string;
declare const accessToken: string;
declare function btoa(input: string): string;
declare const config: Record<string, Record<string, Record<string, string>>>;
declare const credentials: Record<string, string>;
declare const purchasesConfiguration: Record<string, string>;
declare const pagerDutyIntegrationKey: string;
declare const dbPassword: string;
declare const pem: string;
declare const key: string;
declare function getExpoTokenFromSecureStore(): string;
type ExpoAccessTokenConfiguration = string;
type PagerDutyIntegrationKeyReference = string;
type LinkedInClientSecretReference = string;
