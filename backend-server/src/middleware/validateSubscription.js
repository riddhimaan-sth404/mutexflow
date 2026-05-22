function validateSubscription(licenseKey) {
  if (!licenseKey || typeof licenseKey !== "string") {
    return false;
  }
  return true;
}

module.exports = { validateSubscription };
