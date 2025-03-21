
export function api_baseurl() {
    const hostname = window.location.hostname;
    const port = ["", "80", "443"].includes(window.location.port) ? "" : `:${window.location.port}`;
    const protocol = window.location.protocol;
    return `${protocol}//${hostname}${port}`
}
export async function create_access_token_api_baseurl(base_url, auth0) {
    return base_url.includes("localhost") ?
        await auth0.getAccessTokenWithPopup({
            authorizationParams: {
                audience: `${base_url}/api/`,
                scope: "read:current_user",
                redirect_uri: `${base_url}`,
            },
        })
        : await auth0.getAccessTokenSilently({
            authorizationParams: {
                audience: `${base_url}/api/`,
                scope: "read:current_user",
            },
        });
}