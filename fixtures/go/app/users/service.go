package users

// Registers accounts and notifies them.

type UserService struct{}

func (s *UserService) Register(notifications NotificationService, user User) string {
	return notifications.Send(user.ID, "welcome")
}
