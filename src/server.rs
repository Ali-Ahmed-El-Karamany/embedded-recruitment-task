use crate::message::{ClientMessage, ServerMessage, AddResponse, client_message, server_message};
use log::{error, info, warn};
use prost::Message;
use std::{
    io::{self, ErrorKind, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
        Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

struct Client {
    stream: TcpStream,
}

impl Client {
    pub fn new(stream: TcpStream) -> Self {
        Client { stream }
    }

    // Handles incoming client messages and send appropriate response
    pub fn handle(&mut self) -> io::Result<()> {

        let mut buffer = [0; 512];
        // Read data from the client
        let bytes_read = self.stream.read(&mut buffer)?;
        if bytes_read == 0 {
            info!("Client disconnected.");
            return Ok(());
        }

        // Decode the client message check if it is EchoMessage or AddRequest
        if let Ok(client_message) = ClientMessage::decode(&buffer[..bytes_read]) {
            match client_message.message {
                // Handles the Echo message
                Some(client_message::Message::EchoMessage(echo)) => {
                    info!("Received EchoMessage: {}", echo.content);
                    
                    // Creates and send Echo response
                    let response = ServerMessage {
                        message: Some(server_message::Message::EchoMessage(echo)),
                    };
                    self.stream.write_all(&response.encode_to_vec())?;
                }

                // Handles the Add Request 
                Some(client_message::Message::AddRequest(add)) => {
                    info!("Received add request of {} + {}", add.a, add.b);    
                    let result = add.a + add.b;
                    
                    // Create and send addition response 
                    let response = ServerMessage {
                        message: Some(server_message::Message::AddResponse(AddResponse {
                            result,
                        })),
                    };
                    self.stream.write_all(&response.encode_to_vec())?;
                }
                None => {
                    error!("Received message with no content");
                }
            }
            self.stream.flush()?;
        }
        Ok(())
    }
}

// Main server structure, that manges the client connections and server lifecycle
pub struct Server {
    listener: TcpListener,
    is_running: Arc<AtomicBool>,
    client_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Server {
    // creates a server and bind it with a specific address
    pub fn new(addr: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let is_running = Arc::new(AtomicBool::new(false));
        let client_threads = Arc::new(Mutex::new(Vec::new()));
        Ok(Server {
            listener,
            is_running,
            client_threads,
        })
    }

    // Adds a new client thread handle to the managed threads vector
    fn add_client_thread(&self, handle: JoinHandle<()>) {
        if let Ok(mut threads) = self.client_threads.lock() {
            threads.push(handle);
        }
        else {
            error!("failed to store the handle of client thread");
        }
    }

    // Remove completed client threads from the threads vector
    fn finished_threads_cleanup(&self)
    {
        if let Ok(mut threads) = self.client_threads.lock() {
            threads.retain(|threads| !threads.is_finished());
        }
    }

    pub fn run(&self) -> io::Result<()> {
        self.is_running.store(true, Ordering::SeqCst);
        info!("Server is running on {}", self.listener.local_addr()?);

        // Set listener nonblocking mode to true to:
        // 1- Allow checking of is_running flag
        // 2- Prevent the the thread from blocking when no clients are connected
        self.listener.set_nonblocking(true)?;

        while self.is_running.load(Ordering::SeqCst) {
            // cleanup completed threads
            self.finished_threads_cleanup();

            match self.listener.accept() {
                Ok((stream, addr)) => {
                    info!("New client connected: {}", addr);

                    // Set the client stream to blocking mode to ensure we read complete messages from the client
                    stream.set_nonblocking(false)?;
 
                    let is_running = Arc::clone(&self.is_running);

                    // Spwan new thread for client handling    
                    let handle = thread::spawn(move || {
                        let mut client = Client::new(stream);
                        while is_running.load(Ordering::SeqCst) {
                            if let Err(e) = client.handle() {
                                error!("Error handling client: {}", e);
                                break;
                            }
                        }
                    });

                    // Add the thread handle to the vector
                    self.add_client_thread(handle);
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    error!("Error accepting connection: {}", e);
                }
            }
        }

        info!("Server stopped.");
        Ok(())
    }

    // Gracefully stops the server and all client handlers
    // 1. Sets running flag to false
    // 2. Waits for all client threads to complete
    pub fn stop(&self) {
        if self.is_running.load(Ordering::SeqCst) {
            // stop the server from accepting new clients
            self.is_running.store(false, Ordering::SeqCst);
            
            // Join all client threads
            if let Ok(mut threads) = self.client_threads.lock() {
                for thread in threads.drain(..) {
                    if let Err(e) = thread.join() {
                        error!("Error joining client thread: {:?}", e);
                    }
                }
            }

            info!("Shutdown signal sent.");
        } else {
            warn!("Server was already stopped or not running.");
        }
    }
}