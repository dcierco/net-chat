#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Mutex;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_register_user() {
        let user_db = Arc::new(Mutex::new(HashMap::new()));
        let server = Server {
            user_db,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            online_users: Arc::new(Mutex::new(HashMap::new())),
            file_transfers: Arc::new(Mutex::new(HashMap::new())),
        };

        let username = "testuser".to_string();
        let password = "password123".to_string();

        let result = server.register_user(username.clone(), password.clone()).await;
        assert!(result.is_ok());

        // Trying to register the same user again should fail
        let result_duplicate = server.register_user(username, password).await;
        assert!(result_duplicate.is_err());
    }

    #[tokio::test]
    async fn test_authenticate_user() {
        let user_db = Arc::new(Mutex::new(HashMap::new()));
        user_db.lock().await.insert("testuser".to_string(), "password123".to_string());

        let server = Server {
            user_db,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            online_users: Arc::new(Mutex::new(HashMap::new())),
            file_transfers: Arc::new(Mutex::new(HashMap::new())),
        };

        let result = server.authenticate_user("testuser".to_string(), "password123".to_string()).await;
        assert!(result.is_ok());
        
        let wrong_password_result = server.authenticate_user("testuser".to_string(), "wrongpass".to_string()).await;
        assert!(wrong_password_result.is_err());

        let nonexistent_user_result = server.authenticate_user("nonexistent".to_string(), "password".to_string()).await;
        assert!(nonexistent_user_result.is_err());
    }

    #[tokio::test]
    async fn test_list_users() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        sessions.lock().await.insert("testuser".to_string(), "token123".to_string());

        let online_users = Arc::new(Mutex::new(HashMap::new()));
        online_users.lock().await.insert("testuser".to_string(), Arc::new(Mutex::new(TcpStream::default())));

        let server = Server {
            user_db: Arc::new(Mutex::new(HashMap::new())),
            sessions,
            online_users,
            file_transfers: Arc::new(Mutex::new(HashMap::new())),
        };

        let result = server.list_users("testuser".to_string()).await;
        assert_eq!(result, "LIST_USERS|SUCCESS|testuser");
    }
}