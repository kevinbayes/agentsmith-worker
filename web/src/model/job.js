import {logger} from "../utils/logging.js";
import retry from "../utils/retry.js";
import {api_baseurl, create_access_token_api_baseurl} from "../utils/api.js";

export class Job {
    constructor() {
        this.level = import.meta.env.VITE_APP_LOGGING_LEVEL || 'INFO';
    }
}


export class JobStore {

    constructor(auth0) {
        this.baseUrl = import.meta.env.VITE_APP_DH_API_BASE_URL || "";
        this.auth0 = auth0;
        logger.info("Loading job store.", this.baseUrl)
    }

    getJson = async (url, options) => {

        if(options?.params) {
            const queryParams = new URLSearchParams(options.params);
            url = `${url}?${queryParams}`;
        }

        const response = await retry(() => fetch(url, {
            method: 'GET',
        }));

        const result = {
            status: response.status,
            data: await response.json(),
        }

        return result;
    }

    deleteCall = async (url, options) => {

        if(options?.params) {
            const queryParams = new URLSearchParams(options.params);
            url = `${url}?${queryParams}`;
        }

        const response = await retry(() => fetch(url, {
            method: 'DELETE',
        }));

        const result = {
            status: response.status,
            data: await response.json(),
        }

        return result;
    }


    jobs() {
        return this.getJson('/api/jobs');
    }

    job(id) {
        return this.getJson(`/api/jobs/${id}`);
    }

    stop(id) {
        return this.deleteCall(`/api/jobs/${id}`);
    }
}


