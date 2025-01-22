use crate::message::{ClientMessage, ServerMessage, AddResponse, client_message, server_message};
use log::{error, info, warn};
use prost::Message;
use std::{
    io::{self, ErrorKind, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
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

pub struct Server {
    listener: TcpListener,
    is_running: Arc<AtomicBool>,
}

// Main server structure, that manges the client connections and server lifecycle
impl Server {
    /// Creates a new server instance
    pub fn new(addr: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let is_running = Arc::new(AtomicBool::new(false));
        Ok(Server {
            listener,
            is_running,
        })
    }

    /// Runs the server, listening for incoming connections and handling them
    pub fn run(&self) -> io::Result<()> {
        self.is_running.store(true, Ordering::SeqCst); // Set the server as running
        info!("Server is running on {}", self.listener.local_addr()?);

        // Set the listener to non-blocking mode
        self.listener.set_nonblocking(true)?;

        while self.is_running.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((stream, addr)) => {
                    info!("New client connected: {}", addr);

                    // Set the client stream to blocking mode to ensure we read complete messages from the client
                    stream.set_nonblocking(false)?;

                    // Handle the client request
                    let mut client = Client::new(stream);
                    while self.is_running.load(Ordering::SeqCst) {
                        if let Err(e) = client.handle() {
                            error!("Error handling client: {}", e);
                            break;
                        }
                    }
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    // No incoming connections, sleep briefly to reduce CPU usage
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

    /// Stops the server by setting the `is_running` flag to `false`
    pub fn stop(&self) {
        if self.is_running.load(Ordering::SeqCst) {
            self.is_running.store(false, Ordering::SeqCst);
            info!("Shutdown signal sent.");
        } else {
            warn!("Server was already stopped or not running.");
        }
    }
}
