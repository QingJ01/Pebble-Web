// A Web deployment is isolated by its origin, not a native profile directory.
export const profileLocalStorage = window.localStorage;
export const profileSessionStorage = window.sessionStorage;
