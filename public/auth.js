// Passkey (WebAuthn) registration and sign-in. Talks to the /auth/* endpoints,
// converting between the server's base64url JSON and the ArrayBuffers the
// browser's credential API expects.

const errorBox = document.getElementById("auth-error");

function showError(message) {
  if (!errorBox) return;
  errorBox.textContent = message;
  errorBox.hidden = false;
}

function clearError() {
  if (!errorBox) return;
  errorBox.hidden = true;
  errorBox.textContent = "";
}

function b64urlToBuf(value) {
  const pad = "=".repeat((4 - (value.length % 4)) % 4);
  const base64 = (value + pad).replace(/-/g, "+").replace(/_/g, "/");
  const raw = atob(base64);
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i += 1) bytes[i] = raw.charCodeAt(i);
  return bytes.buffer;
}

function bufToB64url(buffer) {
  const bytes = new Uint8Array(buffer);
  let str = "";
  for (const byte of bytes) str += String.fromCharCode(byte);
  return btoa(str).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function prepCreation(publicKey) {
  publicKey.challenge = b64urlToBuf(publicKey.challenge);
  publicKey.user.id = b64urlToBuf(publicKey.user.id);
  if (publicKey.excludeCredentials) {
    publicKey.excludeCredentials = publicKey.excludeCredentials.map((cred) => ({
      ...cred,
      id: b64urlToBuf(cred.id),
    }));
  }
  return publicKey;
}

function prepRequest(publicKey) {
  publicKey.challenge = b64urlToBuf(publicKey.challenge);
  if (publicKey.allowCredentials) {
    publicKey.allowCredentials = publicKey.allowCredentials.map((cred) => ({
      ...cred,
      id: b64urlToBuf(cred.id),
    }));
  }
  return publicKey;
}

function encodeAttestation(credential) {
  return {
    id: credential.id,
    rawId: bufToB64url(credential.rawId),
    type: credential.type,
    extensions: credential.getClientExtensionResults(),
    response: {
      attestationObject: bufToB64url(credential.response.attestationObject),
      clientDataJSON: bufToB64url(credential.response.clientDataJSON),
    },
  };
}

function encodeAssertion(credential) {
  const { response } = credential;
  return {
    id: credential.id,
    rawId: bufToB64url(credential.rawId),
    type: credential.type,
    extensions: credential.getClientExtensionResults(),
    response: {
      authenticatorData: bufToB64url(response.authenticatorData),
      clientDataJSON: bufToB64url(response.clientDataJSON),
      signature: bufToB64url(response.signature),
      userHandle: response.userHandle ? bufToB64url(response.userHandle) : null,
    },
  };
}

async function postJson(url, body) {
  const res = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    throw new Error(data.error || `request failed (${res.status})`);
  }
  return data;
}

async function register(username, displayName) {
  const challenge = await postJson("/auth/register/begin", {
    username,
    display_name: displayName || null,
  });
  // Dev mode (PASSKEY_DISABLED): server signs us in, no ceremony to run.
  if (!challenge.publicKey) return;
  const credential = await navigator.credentials.create({
    publicKey: prepCreation(challenge.publicKey),
  });
  await postJson("/auth/register/finish", encodeAttestation(credential));
}

async function login(username) {
  const challenge = await postJson("/auth/login/begin", { username });
  if (!challenge.publicKey) return;
  const credential = await navigator.credentials.get({
    publicKey: prepRequest(challenge.publicKey),
  });
  await postJson("/auth/login/finish", encodeAssertion(credential));
}

function wire(form, handler) {
  if (!form) return;
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    clearError();
    const button = form.querySelector("button[type=submit]");
    if (button) button.disabled = true;
    try {
      await handler(new FormData(form));
      window.location.assign("/");
    } catch (err) {
      showError(err && err.message ? err.message : "passkey ceremony failed");
    } finally {
      if (button) button.disabled = false;
    }
  });
}

if (!window.PublicKeyCredential) {
  showError("This browser does not support passkeys (WebAuthn).");
}

wire(document.getElementById("register-form"), (data) =>
  register(
    (data.get("username") || "").trim(),
    (data.get("display_name") || "").trim(),
  ),
);

wire(document.getElementById("login-form"), (data) =>
  login((data.get("username") || "").trim()),
);
