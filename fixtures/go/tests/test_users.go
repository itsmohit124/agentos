package tests

import (
	"app/notifications/service"
	"app/users/models"
	"app/users/service"
)

func test_register_sends_notification() {
	user := User{}
	notifications := NotificationService{}
	service := UserService{}
	service.Register(notifications, user)
}
