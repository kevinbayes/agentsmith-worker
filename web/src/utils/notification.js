import { Subject } from 'rxjs';

export class NotificationService {

    subject = new Subject();

    registerStateCallback(callback) {
        return this.subject.subscribe(callback);
    }

    update(notification) {
        this.subject.next(notification);
    }

    httpError(error) {
        if(error.response?.data?.message) {
            const response = error.response.data;
            this.subject.next({error: `${response.error} - ${response.message}`});
        } else {
            this.subject.next({error: `${error.message}`});
        }
    }

    error(notification) {
        this.subject.next({ error: notification });
    }

    success(notification) {
        this.subject.next({ success: notification });
    }


}

export const notificationService = new NotificationService();
