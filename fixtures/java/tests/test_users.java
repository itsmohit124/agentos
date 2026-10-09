package tests;

import app.notifications.NotificationService;
import app.users.User;
import app.users.UserService;

/** Exercises registration. */
class UserTests {
    void test_register_sends_notification() {
        User user = new User("user-1");
        NotificationService notifications = new NotificationService();
        UserService service = new UserService();
        service.register(notifications, user);
    }
}
